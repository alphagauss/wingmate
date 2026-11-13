package dev.wingmate.mobile.web

import android.Manifest
import android.annotation.SuppressLint
import android.content.ActivityNotFoundException
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.content.pm.PackageManager
import android.content.res.Configuration
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.view.ViewGroup
import android.webkit.CookieManager
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.FrameLayout
import android.widget.Toast
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import dev.wingmate.mobile.i18n.ConnectionStrings
import dev.wingmate.mobile.localEngineSettings
import dev.wingmate.mobile.remote.ConnectionFlow
import dev.wingmate.mobile.remote.addManagementButton
import dev.wingmate.mobile.settings.AppPreferences
import dev.wingmate.mobile.settings.AppSettings
import dev.wingmate.mobile.theme.ConnectionUi
import dev.wingmate.mobile.theme.applyWindowInsets

internal const val START_LOCAL_WORKBENCH = "startLocalWorkbench"

class WorkbenchActivity : AppCompatActivity() {
    private val labels by lazy { ConnectionStrings(this) }
    private val preferences by lazy { AppPreferences(this) }
    private lateinit var webView: WebView
    private lateinit var bridge: WorkbenchBridge
    private lateinit var loading: WorkbenchLoading
    private lateinit var appSettings: AppSettings
    private lateinit var activeOrigin: String
    private lateinit var workbench: FrameLayout
    private lateinit var root: FrameLayout
    private var loadFailed = false
    private var paused = true
    private var localToken: String? = null
    private val notifications = registerForActivityResult(ActivityResultContracts.RequestPermission()) {}
    private val connections = ConnectionFlow(this)
    private var fileResult: ValueCallback<Array<Uri>>? = null
    private val filePicker = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        fileResult?.onReceiveValue(WebChromeClient.FileChooserParams.parseResult(it.resultCode, it.data))
        fileResult = null
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        workbench = FrameLayout(this)
        root = FrameLayout(this).apply {
            setBackgroundColor(ConnectionUi(this@WorkbenchActivity).background)
            addView(workbench, FrameLayout.LayoutParams(-1, -1))
            applyWindowInsets()
        }
        loading = WorkbenchLoading(this, ::retry) { appSettings.open() }
        workbench.addView(loading, FrameLayout.LayoutParams(-1, -1))
        setContentView(root)
        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                when {
                    !::appSettings.isInitialized -> finish()
                    appSettings.back() -> Unit
                    webView.canGoBack() -> webView.goBack()
                    else -> finish()
                }
            }
        })
        val link = intent.getStringExtra("link")
        if (link != null) {
            showWorkbench(link)
        } else if (intent.getBooleanExtra(START_LOCAL_WORKBENCH, false)) {
            startLocalWorkbench()
        } else {
            finish()
        }
    }

    private fun startLocalWorkbench() {
        val engine = localEngineSettings(this) { link ->
            intent.removeExtra(START_LOCAL_WORKBENCH)
            intent.putExtra("link", link.toString())
            showWorkbench(link.toString(), fromStartup = true)
        } ?: return finish()
        engine.openWorkbench?.let { open ->
            open()
            return
        }
        if (!engine.shouldStartOnLaunch) return finish()
        loading.start(labels["localAgent.starting"])
        engine.startOnLaunch()
        engine.observe {
            if (!intent.getBooleanExtra(START_LOCAL_WORKBENCH, false) || isFinishing) return@observe
            val open = engine.openWorkbench
            if (open != null) {
                open()
            } else if (!engine.launchPending) {
                Toast.makeText(this, engine.summary, Toast.LENGTH_LONG).show()
                finish()
            }
        }
    }

    @SuppressLint("SetJavaScriptEnabled")
    private fun showWorkbench(link: String, fromStartup: Boolean = false) {
        val origin = Uri.parse(link)
        activeOrigin = origin.workbenchOrigin() ?: return finish()
        val localCurrent = intent.getBooleanExtra("localAgent", false)
        if (localCurrent) localToken = origin.getQueryParameter("token")
        val ui = ConnectionUi(this)
        // Workbench events are scoped to the connected origin.
        if (applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE != 0) {
            WebView.setWebContentsDebuggingEnabled(true)
        }
        webView = WebView(this).apply {
            setBackgroundColor(ui.background)
            settings.javaScriptEnabled = true
            settings.domStorageEnabled = true
            settings.allowFileAccess = false
            webViewClient = workbenchClient(origin)
            webChromeClient = object : WebChromeClient() {
                override fun onShowFileChooser(
                    view: WebView, callback: ValueCallback<Array<Uri>>, params: FileChooserParams
                ): Boolean {
                    fileResult?.onReceiveValue(null)
                    fileResult = callback
                    try {
                        filePicker.launch(params.createIntent())
                    } catch (_: ActivityNotFoundException) {
                        fileResult = null
                        callback.onReceiveValue(null)
                    }
                    return true
                }
            }
        }
        workbench.addView(webView, 0, FrameLayout.LayoutParams(-1, -1))

        root.removeView(workbench)
        appSettings = AppSettings(this, workbench, connections, activeOrigin,
            localCurrent, localEngineSettings(this, ::openLocalWorkbench))
        bridge = WorkbenchBridge(this, webView, activeOrigin, ::ready) { theme ->
            appSettings.applyTheme(theme)
            loading.applyTheme(theme)
        }
        root.addView(appSettings.view, FrameLayout.LayoutParams(-1, -1))
        workbench.addManagementButton(getSharedPreferences("connection", MODE_PRIVATE), appSettings::open)
        if (fromStartup) loading.enter()
        webView.loadUrl(link)
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        if (::appSettings.isInitialized) appSettings.onConfigurationChanged()
    }

    private fun retry() {
        intent.getStringExtra("link")?.let(webView::loadUrl)
    }

    private fun openLocalWorkbench(link: Uri) {
        appSettings.close()
        if (link.workbenchOrigin() == activeOrigin && intent.getBooleanExtra("localAgent", false)) {
            val token = link.getQueryParameter("token")
            if (localToken != token || loadFailed) {
                localToken = token
                intent.putExtra("link", link.toString())
                retry()
            }
            return
        }
        startActivity(Intent(this, WorkbenchActivity::class.java)
            .putExtra("link", link.toString()).putExtra("localAgent", true))
        finish()
    }

    private fun ready() {
        if (loadFailed) return
        loading.stop()
        if (paused) return
        applyNotificationFocus()
        if (preferences.taskNotifications && Build.VERSION.SDK_INT >= 33 &&
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            val preferences = getSharedPreferences("connection", MODE_PRIVATE)
            if (!preferences.getBoolean("notifications_requested", false)) {
                preferences.edit().putBoolean("notifications_requested", true).apply()
                notifications.launch(Manifest.permission.POST_NOTIFICATIONS)
            }
        }
    }

    private fun applyNotificationFocus() {
        val session = intent.getStringExtra("notificationSession") ?: return
        if (bridge.focus(session)) {
            intent.removeExtra("notificationSession")
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        val link = Uri.parse(intent.getStringExtra("link") ?: return)
        val next = link.workbenchOrigin() ?: return
        if (!::activeOrigin.isInitialized || next != activeOrigin) {
            startActivity(Intent(this, WorkbenchActivity::class.java).putExtras(intent))
            finish()
        } else {
            setIntent(intent)
            if (intent.getBooleanExtra("localAgent", false) && link.getQueryParameter("token") != null) retry()
            else applyNotificationFocus()
        }
    }

    private fun workbenchClient(origin: Uri) = object : WebViewClient() {
        private val pairingParameter = if (intent.getBooleanExtra("localAgent", false)) "token" else "auth"
        private var clearPairingHistory = origin.getQueryParameter(pairingParameter) != null
        private var authenticating = false

        private fun sameOrigin(uri: Uri): Boolean =
            uri.scheme == origin.scheme && uri.host == origin.host && uri.port == origin.port && uri.userInfo == null

        override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
            if (!request.isForMainFrame) return false
            val destination = request.url
            if (sameOrigin(destination)) {
                authenticating = false
                return false
            }
            if (destination.scheme == "https" && destination.userInfo == null &&
                (authenticating || (request.isRedirect && destination.host.orEmpty().endsWith(".cloudflareaccess.com") &&
                    destination.path.orEmpty().startsWith("/cdn-cgi/access/login/")))
            ) {
                authenticating = true
                clearPairingHistory = true
                loading.stop()
                return false
            }
            if (destination.scheme in listOf("http", "https") && destination.userInfo == null) {
                try {
                    startActivity(Intent(Intent.ACTION_VIEW, destination))
                } catch (_: ActivityNotFoundException) {
                    Toast.makeText(this@WorkbenchActivity, labels["connection.connectionFailed"], Toast.LENGTH_SHORT).show()
                }
            }
            return true
        }

        override fun onPageStarted(view: WebView, url: String, favicon: android.graphics.Bitmap?) {
            val current = Uri.parse(url)
            if (!sameOrigin(current)) return
            if (pairingParameter == "token" && current.path == "/" && current.getQueryParameter("token") != null) {
                clearPairingHistory = true
            }
            loadFailed = false
            bridge.started()
            if (!loading.entering) loading.start()
        }

        override fun onPageFinished(view: WebView, url: String) {
            val current = Uri.parse(url)
            if (!sameOrigin(current)) return
            bridge.finished()
            if (clearPairingHistory && current.getQueryParameter(pairingParameter) == null) {
                view.clearHistory()
                CookieManager.getInstance().flush()
                clearPairingHistory = false
                // The Intent is retained on Activity recreation; keep only the origin after pairing.
                intent.putExtra("link", origin.buildUpon().clearQuery().build().toString())
            }
        }

        override fun onReceivedError(view: WebView, request: WebResourceRequest, error: WebResourceError) {
            if (request.isForMainFrame) failed()
        }

        override fun onReceivedHttpError(view: WebView, request: WebResourceRequest, response: WebResourceResponse) {
            if (request.isForMainFrame && response.statusCode >= 400 && response.statusCode != 401) failed()
        }

        private fun failed() {
            loadFailed = true
            loading.error(labels["connection.connectionFailed"])
        }
    }

    override fun onResume() {
        super.onResume()
        paused = false
        if (::bridge.isInitialized) {
            appSettings.refresh()
            applyNotificationFocus()
        }
    }

    override fun onPause() {
        paused = true
        super.onPause()
    }

    override fun onStop() {
        CookieManager.getInstance().flush()
        super.onStop()
    }

    override fun onDestroy() {
        fileResult?.onReceiveValue(null)
        if (::webView.isInitialized) {
            (webView.parent as? ViewGroup)?.removeView(webView)
            webView.destroy()
        }
        super.onDestroy()
    }
}

private fun Uri.workbenchOrigin(): String? =
    if ((scheme == "http" || scheme == "https") && !host.isNullOrEmpty() && userInfo == null) {
        "$scheme://$encodedAuthority"
    } else null
