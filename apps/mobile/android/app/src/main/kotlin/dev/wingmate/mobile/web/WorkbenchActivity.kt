package dev.wingmate.mobile.web

import android.Manifest
import android.annotation.SuppressLint
import android.content.ActivityNotFoundException
import android.content.Intent
import android.content.pm.PackageManager
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
import dev.wingmate.mobile.localAgentAction
import dev.wingmate.mobile.remote.ConnectionDrawer
import dev.wingmate.mobile.remote.ConnectionFlow
import dev.wingmate.mobile.remote.addManagementButton
import dev.wingmate.mobile.theme.ConnectionUi
import dev.wingmate.mobile.theme.applyWindowInsets

class WorkbenchActivity : AppCompatActivity() {
    private val labels by lazy { ConnectionStrings(this) }
    private lateinit var webView: WebView
    private lateinit var bridge: WorkbenchBridge
    private lateinit var loading: WorkbenchLoading
    private lateinit var activeOrigin: String
    private var loadFailed = false
    private var paused = true
    private val notifications = registerForActivityResult(ActivityResultContracts.RequestPermission()) {}
    private val connections = ConnectionFlow(this)
    private var fileResult: ValueCallback<Array<Uri>>? = null
    private val filePicker = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) {
        fileResult?.onReceiveValue(WebChromeClient.FileChooserParams.parseResult(it.resultCode, it.data))
        fileResult = null
    }

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        val link = intent.getStringExtra("link") ?: return finish()
        val origin = Uri.parse(link)
        activeOrigin = origin.workbenchOrigin() ?: return finish()
        val ui = ConnectionUi(this)
        val workbench = FrameLayout(this)
        val root = FrameLayout(this).apply {
            setBackgroundColor(ui.background)
            applyWindowInsets()
        }
        // Workbench events are scoped to the connected origin.
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
        workbench.addView(webView, FrameLayout.LayoutParams(-1, -1))

        val drawer = ConnectionDrawer(this, workbench, connections, activeOrigin,
            localAgentAction(this), intent.getBooleanExtra("localAgent", false))
        loading = WorkbenchLoading(this, ::retry, drawer::open)
        workbench.addView(loading, FrameLayout.LayoutParams(-1, -1))
        bridge = WorkbenchBridge(this, webView, activeOrigin, ::ready) { theme ->
            drawer.applyTheme(theme)
            loading.applyTheme(theme)
        }
        root.addView(drawer, FrameLayout.LayoutParams(-1, -1))
        workbench.addManagementButton(getSharedPreferences("connection", MODE_PRIVATE), drawer::open)
        setContentView(root)
        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                when {
                    drawer.showing() -> drawer.close()
                    webView.canGoBack() -> webView.goBack()
                    else -> finish()
                }
            }
        })
        webView.loadUrl(link)
    }

    private fun retry() {
        intent.getStringExtra("link")?.let(webView::loadUrl)
    }

    private fun ready() {
        if (loadFailed) return
        loading.stop()
        bridge.visible(!paused)
        if (paused) return
        applyNotificationFocus()
        if (Build.VERSION.SDK_INT >= 33 && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) !=
            PackageManager.PERMISSION_GRANTED
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
        if (bridge.focus(session, intent.getStringExtra("notificationTag").orEmpty())) {
            intent.removeExtra("notificationSession")
            intent.removeExtra("notificationTag")
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        val link = Uri.parse(intent.getStringExtra("link") ?: return)
        val next = link.workbenchOrigin() ?: return
        if (next != activeOrigin) {
            startActivity(Intent(this, WorkbenchActivity::class.java).putExtras(intent)
                .addFlags(Intent.FLAG_ACTIVITY_FORWARD_RESULT))
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
            loading.start()
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
            bridge.visible(true)
            applyNotificationFocus()
        }
    }

    override fun onPause() {
        paused = true
        if (::bridge.isInitialized) bridge.visible(false)
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
