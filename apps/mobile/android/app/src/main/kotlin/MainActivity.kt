package dev.wingmate.mobile

import android.annotation.SuppressLint
import android.content.Intent
import android.content.res.Configuration
import android.os.Bundle
import android.view.ViewGroup
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge
import androidx.appcompat.app.AppCompatActivity
import androidx.webkit.WebViewAssetLoader
import dev.wingmate.mobile.launch.ConnectionBridge
import dev.wingmate.mobile.remote.ConnectionFlow
import dev.wingmate.mobile.settings.AppPreferences
import dev.wingmate.mobile.settings.AppSettings
import dev.wingmate.mobile.theme.applyWindowInsets
import dev.wingmate.mobile.web.WorkbenchActivity
import java.io.ByteArrayInputStream

// Keep the previous local WebView origin to import saved connections from older APKs.
private const val LAUNCH_HOST = "tauri.localhost"
private const val LAUNCH_PATH = "/assets/launch/"
private const val CONNECTION_BRIDGE = "wingmateConnection"
internal const val OPEN_HOST_SELECTION = "openHostSelection"

class MainActivity : AppCompatActivity() {
    private lateinit var webView: WebView
    private lateinit var bridge: ConnectionBridge
    private lateinit var appSettings: AppSettings
    private var openLocalOnLaunch = false
    private val connections = ConnectionFlow(this, onConnectionRequested = { bridge.cancelDiscovery() })

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        val openOnLaunch = savedInstanceState == null &&
            !intent.getBooleanExtra(OPEN_HOST_SELECTION, false) && !intent.getBooleanExtra("openLocalAgent", false)
        val localAgent = localEngineSettings(this) { link ->
            appSettings.close()
            startActivity(Intent(this, WorkbenchActivity::class.java)
                .putExtra("link", link.toString()).putExtra("localAgent", true))
        }

        val assets = WebViewAssetLoader.Builder()
            .setDomain(LAUNCH_HOST)
            .setHttpAllowed(true)
            .addPathHandler("/assets/", WebViewAssetLoader.AssetsPathHandler(this))
            .build()
        webView = WebView(this).apply {
            settings.javaScriptEnabled = true
            settings.domStorageEnabled = true
            settings.allowFileAccess = false
            settings.allowContentAccess = false
            webViewClient = object : WebViewClient() {
                override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest) = true

                override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse {
                    val url = request.url
                    if (url.scheme == "http" && url.host == LAUNCH_HOST && url.port == -1 &&
                        url.path?.startsWith(LAUNCH_PATH) == true && url.pathSegments.size == 3
                    ) {
                        assets.shouldInterceptRequest(url)?.let { return it }
                    }
                    return WebResourceResponse("text/plain", "UTF-8", 404, "Not Found", emptyMap(),
                        ByteArrayInputStream(byteArrayOf()))
                }
            }
        }
        appSettings = AppSettings(this, webView, connections, localEngine = localAgent,
            languageChanged = { bridge.updateLabels() })
        val startLocal = openOnLaunch && localAgent?.startOnLaunch() == true
        val autoConnect = openOnLaunch && AppPreferences(this).autoConnect
        openLocalOnLaunch = autoConnect && (startLocal || localAgent?.openWorkbench != null)
        bridge = ConnectionBridge(this, webView, connections, appSettings::open, appSettings::openRemote,
            localAgent, appSettings::openLocal, autoConnect && !openLocalOnLaunch) { openLocalOnLaunch = false }
        webView.addJavascriptInterface(bridge, CONNECTION_BRIDGE)
        setContentView(FrameLayout(this).apply {
            addView(appSettings.view, FrameLayout.LayoutParams(-1, -1))
            applyWindowInsets()
        })
        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                if (!appSettings.back()) {
                    isEnabled = false
                    onBackPressedDispatcher.onBackPressed()
                    isEnabled = true
                }
            }
        })
        webView.loadUrl("http://$LAUNCH_HOST${LAUNCH_PATH}index.html")
        localAgent?.observe {
            bridge.updateLocalEntry()
            if (openLocalOnLaunch) localAgent.openWorkbench?.let { open ->
                openLocalOnLaunch = false
                open()
            }
        }
        openRequestedSettings(intent)
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        appSettings.onConfigurationChanged()
        bridge.updateLabels()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        openRequestedSettings(intent)
    }

    override fun onStop() {
        openLocalOnLaunch = false
        bridge.cancelStartupConnection()
        super.onStop()
    }

    override fun onResume() {
        super.onResume()
        appSettings.refresh()
    }

    private fun openRequestedSettings(intent: Intent) {
        when {
            intent.getBooleanExtra(OPEN_HOST_SELECTION, false) -> {
                openLocalOnLaunch = false
                bridge.cancelStartupConnection()
                intent.removeExtra(OPEN_HOST_SELECTION)
                appSettings.showHostSelection()
            }
            intent.getBooleanExtra("openLocalAgent", false) -> {
                openLocalOnLaunch = false
                bridge.cancelStartupConnection()
                intent.removeExtra("openLocalAgent")
                appSettings.openLocal()
            }
        }
    }

    override fun onDestroy() {
        bridge.cancelDiscovery()
        webView.removeJavascriptInterface(CONNECTION_BRIDGE)
        (webView.parent as ViewGroup).removeView(webView)
        webView.destroy()
        super.onDestroy()
    }
}
