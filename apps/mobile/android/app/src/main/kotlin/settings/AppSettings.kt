package dev.wingmate.mobile.settings

import android.view.View
import androidx.appcompat.app.AppCompatActivity
import dev.wingmate.mobile.R
import dev.wingmate.mobile.i18n.ConnectionStrings
import dev.wingmate.mobile.remote.ConnectionFlow
import dev.wingmate.mobile.remote.RemoteHostSettings

internal class AppSettings(
    activity: AppCompatActivity,
    workbench: View,
    flow: ConnectionFlow,
    current: String? = null,
    private val localCurrent: Boolean = false,
    private val openLocalAgent: (() -> Unit)? = null
) {
    private val labels = ConnectionStrings(activity)
    private var theme = "system"
    private var ui = SettingsUi(activity, theme)
    private val overlay = SettingsOverlay(activity, workbench, ::back)
    val view: View get() = overlay
    private val remote = RemoteHostSettings(activity, flow, current.takeUnless { localCurrent })
    private var page = Page.HOME
    private var firstPage = Page.HOME

    init {
        overlay.addOnAttachStateChangeListener(object : View.OnAttachStateChangeListener {
            override fun onViewAttachedToWindow(v: View) = Unit
            override fun onViewDetachedFromWindow(v: View) = remote.stop()
        })
    }

    fun open() = open(Page.HOME)
    fun openRemote() = open(Page.REMOTE)

    private fun open(start: Page) {
        if (overlay.showing) return
        firstPage = start
        navigate(start)
        overlay.open()
    }

    fun back(): Boolean {
        if (!overlay.showing) return false
        remote.stop()
        if (page != firstPage) {
            navigate(firstPage)
            overlay.resetSwipe()
        } else {
            overlay.close()
        }
        return true
    }

    fun applyTheme(next: String) {
        if (theme == next) return
        theme = next
        updateColors()
    }

    fun onConfigurationChanged() {
        if (theme == "system") updateColors()
    }

    private fun updateColors() {
        ui = SettingsUi(view.context, theme)
        if (overlay.showing && !overlay.closing) render()
    }

    private fun navigate(next: Page) {
        page = next
        render()
    }

    private fun render() {
        val panel = overlay.page
        panel.removeAllViews()
        val content = if (page == Page.REMOTE) {
            ui.page(labels["settings.remoteHosts"], labels["settings.back"], { back() }) {
                remote.attach(ui, this)
            }
        } else {
            ui.page(labels["settings.title"], labels["settings.back"], { back() }) {
                with(ui) {
                    section {
                        if (localCurrent) {
                            addView(row(labels["localAgent.title"], labels["connection.currentConnection"],
                                icon = R.drawable.ic_settings_engine, action = openLocalAgent))
                        } else {
                            addView(row(remote.currentName ?: labels["connection.noConnection"],
                                labels["settings.remoteHosts"], icon = R.drawable.ic_settings_host,
                                action = { navigate(Page.REMOTE) }))
                        }
                    }
                    section(labels["settings.hosts"]) {
                        openLocalAgent?.let { open ->
                            addView(row(labels["localAgent.title"], icon = R.drawable.ic_settings_engine, action = open))
                        }
                        addView(row(labels["settings.remoteHosts"], icon = R.drawable.ic_settings_host,
                            value = remote.count.takeIf { it > 0 }?.toString(), action = { navigate(Page.REMOTE) }))
                    }
                }
            }
        }
        panel.addView(content)
    }

    private enum class Page { HOME, REMOTE }
}
