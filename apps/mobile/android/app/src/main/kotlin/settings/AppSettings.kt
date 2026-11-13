package dev.wingmate.mobile.settings

import android.view.View
import android.view.inputmethod.InputMethodManager
import android.widget.LinearLayout
import android.widget.Toast
import androidx.core.widget.doAfterTextChanged
import androidx.appcompat.app.AppCompatActivity
import dev.wingmate.mobile.R
import dev.wingmate.mobile.i18n.ConnectionStrings
import dev.wingmate.mobile.remote.ConnectionFlow
import dev.wingmate.mobile.remote.RemoteHostSettings
import dev.wingmate.mobile.theme.dp

internal class AppSettings(
    private val activity: AppCompatActivity,
    workbench: View,
    flow: ConnectionFlow,
    current: String? = null,
    private val localCurrent: Boolean = false,
    private val openLocalAgent: (() -> Unit)? = null,
    private val languageChanged: () -> Unit = {},
    private val openDshSettings: ((DshSettings) -> Boolean)? = null
) {
    private val labels = ConnectionStrings(activity)
    private var theme = "system"
    private var ui = SettingsUi(activity, theme)
    private val overlay: SettingsOverlay = SettingsOverlay(activity, workbench, ::back)
    val view: View get() = overlay
    private val remote = RemoteHostSettings(activity, flow, current.takeUnless { localCurrent }, openLocalAgent == null)
    private val general = GeneralSettings(activity) {
        languageChanged()
        render()
    }
    private var dshReady = false
    private var page = Page.HOME
    private var firstPage = Page.HOME
    private var searchQuery = ""
    private var fromSearch = false

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
        fromSearch = false
        searchQuery = ""
        navigate(start)
        overlay.open()
    }

    fun back(): Boolean {
        if (!overlay.showing) return false
        if (page != firstPage) {
            navigate(if (fromSearch) Page.SEARCH else firstPage)
            fromSearch = false
            overlay.resetSwipe()
        } else {
            remote.stop()
            overlay.close()
        }
        return true
    }

    fun setDshReady(ready: Boolean) {
        if (dshReady == ready) return
        dshReady = ready
        if (page == Page.HOME || page == Page.SEARCH) refresh()
    }

    fun dshSettingsOpened(opened: Boolean) {
        if (!overlay.showing || overlay.closing) return
        if (opened) {
            remote.stop()
            overlay.close()
        } else {
            Toast.makeText(activity, labels["settings.webUnavailable"], Toast.LENGTH_SHORT).show()
        }
    }

    fun refresh() {
        if (overlay.showing && !overlay.closing) render()
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
        refresh()
    }

    private fun navigate(next: Page) {
        if (page == Page.SEARCH && next != Page.SEARCH) {
            activity.getSystemService(InputMethodManager::class.java)
                .hideSoftInputFromWindow(view.windowToken, 0)
        }
        page = next
        render()
    }

    private fun render() {
        remote.stop()
        val panel = overlay.page
        panel.removeAllViews()
        panel.addView(ui.page(labels[page.title], labels["settings.back"], { back() }) {
            when (page) {
                Page.HOME -> home(this)
                Page.SEARCH -> search(this)
                Page.REMOTE -> remote.attach(ui, this)
                Page.NOTIFICATIONS -> general.notifications(ui, this)
                Page.PERMISSIONS -> general.permissions(ui, this)
                Page.DIAGNOSTICS -> general.diagnostics(ui, this,
                    if (localCurrent) labels["localAgent.title"] else remote.currentName)
                Page.ABOUT -> general.about(ui, this)
            }
        })
    }

    private fun home(body: LinearLayout) = with(ui) {
        body.addView(searchButton(labels["settings.search"]) { navigate(Page.SEARCH) },
            LinearLayout.LayoutParams(-1, -2).apply { bottomMargin = activity.dp(16) })
        body.section {
            if (localCurrent) {
                addView(row(labels["localAgent.title"], labels["connection.currentConnection"],
                    icon = R.drawable.ic_settings_engine, action = openLocalAgent))
            } else {
                addView(row(remote.currentName ?: labels["connection.noConnection"],
                    labels["settings.remoteHosts"], icon = R.drawable.ic_settings_host,
                    action = { navigate(Page.REMOTE) }))
            }
        }
        for ((group, entries) in groups()) {
            body.section(labels[group]) {
                entries.forEach { item -> addView(row(labels[item.title], value = item.value,
                    icon = item.icon, action = item.action)) }
            }
        }
    }

    private fun search(body: LinearLayout) = with(ui) {
        val input = search(labels["settings.search"])
        input.setText(searchQuery)
        body.addView(input)
        val matches = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(0, activity.dp(16), 0, 0)
        }
        body.addView(matches)
        val items = groups().flatMap { it.second }
        fun update(query: String) {
            matches.removeAllViews()
            val found = items.filter { labels[it.title].contains(query.trim(), ignoreCase = true) }
            matches.section {
                if (found.isEmpty()) addView(row(labels["settings.noResults"]))
                found.forEach { item ->
                    addView(row(labels[item.title], icon = item.icon, action = {
                        item.action()
                        fromSearch = page != Page.SEARCH
                    }))
                }
            }
        }
        input.doAfterTextChanged {
            searchQuery = it.toString()
            update(searchQuery)
        }
        update(searchQuery)
    }

    private fun groups(): List<Pair<String, List<Item>>> = buildList {
        if (dshReady && openDshSettings != null) {
            add("settings.ai" to listOf(webItem(DshSettings.MODELS, R.drawable.ic_settings_models)))
            add("settings.capabilities" to listOf(webItem(DshSettings.PLUGINS, R.drawable.ic_settings_plugins)))
            add("settings.appearance" to listOf(webItem(DshSettings.GENERAL, R.drawable.ic_settings_theme,
                labels["settings.$theme"])))
        }
        add("settings.general" to listOf(
            Item("settings.language", R.drawable.ic_manage, general.languageName, general::language),
            item(Page.NOTIFICATIONS, R.drawable.ic_settings_bell, general.state(general.notificationsEnabled)),
            item(Page.PERMISSIONS, R.drawable.ic_settings_shield)
        ))
        add("settings.hosts" to buildList {
            openLocalAgent?.let { add(Item("localAgent.title", R.drawable.ic_settings_engine, action = it)) }
            add(item(Page.REMOTE, R.drawable.ic_settings_host, remote.count.takeIf { it > 0 }?.toString()))
        })
        add("settings.help" to listOf(
            item(Page.DIAGNOSTICS, R.drawable.ic_settings_engine),
            item(Page.ABOUT, R.drawable.ic_settings_info, general.version)
        ))
    }

    private fun item(page: Page, icon: Int, value: String? = null) =
        Item(page.title, icon, value) { navigate(page) }

    private fun webItem(section: DshSettings, icon: Int, value: String? = null) =
        Item(section.title, icon, value) {
            if (openDshSettings?.invoke(section) == false) dshSettingsOpened(false)
        }

    private data class Item(val title: String, val icon: Int, val value: String? = null, val action: () -> Unit)

    private enum class Page(val title: String) {
        HOME("settings.title"), SEARCH("settings.search"), REMOTE("settings.remoteHosts"),
        NOTIFICATIONS("settings.taskNotifications"), PERMISSIONS("settings.permissions"),
        DIAGNOSTICS("settings.diagnostics"), ABOUT("settings.about")
    }
}

internal enum class DshSettings(val title: String) {
    MODELS("settings.models"), PLUGINS("settings.plugins"), GENERAL("settings.theme")
}
