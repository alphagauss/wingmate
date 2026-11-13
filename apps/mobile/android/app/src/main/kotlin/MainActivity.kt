package dev.wingmate.mobile

import android.content.Intent
import android.content.res.Configuration
import android.os.Bundle
import android.view.View
import android.widget.FrameLayout
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge
import androidx.appcompat.app.AppCompatActivity
import dev.wingmate.mobile.launch.ConnectionHome
import dev.wingmate.mobile.remote.ConnectionFlow
import dev.wingmate.mobile.settings.AppPreferences
import dev.wingmate.mobile.settings.AppSettings
import dev.wingmate.mobile.theme.ConnectionUi
import dev.wingmate.mobile.theme.applyWindowInsets
import dev.wingmate.mobile.web.START_LOCAL_WORKBENCH
import dev.wingmate.mobile.web.WorkbenchActivity

internal const val OPEN_HOST_SELECTION = "openHostSelection"

class MainActivity : AppCompatActivity() {
    private lateinit var home: ConnectionHome
    private lateinit var appSettings: AppSettings
    private val connections = ConnectionFlow(this, onConnectionRequested = { home.cancelDiscovery() })

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

        home = ConnectionHome(this, connections, localAgent, { appSettings.openLocal() },
            { appSettings.openRemote() }, { appSettings.open() })
        appSettings = AppSettings(this, home.view, connections, localEngine = localAgent,
            languageChanged = home::render)
        val autoConnect = openOnLaunch && AppPreferences(this).autoConnect
        val openLocal = autoConnect && (localAgent?.openWorkbench != null || localAgent?.shouldStartOnLaunch == true)
        setContentView(FrameLayout(this).apply {
            setBackgroundColor(ConnectionUi(this@MainActivity).background)
            addView(appSettings.view, FrameLayout.LayoutParams(-1, -1))
            applyWindowInsets()
        })
        onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
            override fun handleOnBackPressed() {
                if (!appSettings.back()) finish()
            }
        })
        localAgent?.observe(home::render)
        if (openLocal) {
            appSettings.view.visibility = View.INVISIBLE
            startActivity(Intent(this, WorkbenchActivity::class.java)
                .putExtra(START_LOCAL_WORKBENCH, true).putExtra("localAgent", true))
        } else if (openOnLaunch && localAgent?.shouldStartOnLaunch == true) {
            localAgent.startOnLaunch()
        }
        if (autoConnect && !openLocal && connections.store.entries().isNotEmpty()) home.discover(true)
        openRequestedSettings(intent)
    }

    override fun onConfigurationChanged(newConfig: Configuration) {
        super.onConfigurationChanged(newConfig)
        appSettings.onConfigurationChanged()
        home.render()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        openRequestedSettings(intent)
    }

    override fun onStop() {
        home.cancelDiscovery()
        super.onStop()
    }

    override fun onResume() {
        super.onResume()
        appSettings.refresh()
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        if (hasFocus) appSettings.view.visibility = View.VISIBLE
    }

    private fun openRequestedSettings(intent: Intent) {
        when {
            intent.getBooleanExtra(OPEN_HOST_SELECTION, false) -> {
                home.cancelDiscovery()
                intent.removeExtra(OPEN_HOST_SELECTION)
                appSettings.showHostSelection()
            }
            intent.getBooleanExtra("openLocalAgent", false) -> {
                home.cancelDiscovery()
                intent.removeExtra("openLocalAgent")
                appSettings.openLocal()
            }
        }
    }

    override fun onDestroy() {
        home.cancelDiscovery()
        super.onDestroy()
    }
}
