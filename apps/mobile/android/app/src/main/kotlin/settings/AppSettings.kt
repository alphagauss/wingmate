package dev.wingmate.mobile.settings

import android.view.View
import android.view.inputmethod.InputMethodManager
import android.widget.LinearLayout
import android.widget.Toast
import androidx.appcompat.app.AppCompatActivity
import androidx.core.widget.doAfterTextChanged
import dev.wingmate.mobile.R
import dev.wingmate.mobile.i18n.ConnectionStrings
import dev.wingmate.mobile.remote.ConnectionFlow
import dev.wingmate.mobile.remote.RemoteHostSettings
import dev.wingmate.mobile.theme.dp

internal class AppSettings(
    private val activity: AppCompatActivity,
    workbench: View,
    private val flow: ConnectionFlow,
    current: String? = null,
    private val localCurrent: Boolean = false,
    private val localEngine: LocalEngineSettings? = null,
    private val languageChanged: () -> Unit = {}
) {
    private val labels = ConnectionStrings(activity)
    private var theme = "system"
    private var ui = SettingsUi(activity, theme)
    private val overlay: SettingsOverlay = SettingsOverlay(activity, workbench, ::back)
    val view: View get() = overlay
    private val remote = RemoteHostSettings(activity, flow, current, localCurrent, localEngine != null)
    private val general = GeneralSettings(activity) {
        languageChanged()
        render()
    }
    private var page = Page.HOME
    private var firstPage = Page.HOME
    private var searchQuery = ""
    private var fromSearch = false

    init {
        localEngine?.observe {
            when (page) {
                Page.HOME, Page.REMOTE, Page.LOCAL -> refresh()
                else -> Unit
            }
        }
        overlay.addOnAttachStateChangeListener(object : View.OnAttachStateChangeListener {
            override fun onViewAttachedToWindow(v: View) = Unit
            override fun onViewDetachedFromWindow(v: View) = remote.stop()
        })
    }

    fun open() = open(Page.HOME)
    fun openRemote() = open(Page.REMOTE)
    fun showHostSelection() = show(Page.REMOTE)
    fun openLocal() {
        if (localEngine == null) return
        if (!overlay.showing) open(Page.LOCAL)
        else if (!overlay.closing) navigate(Page.LOCAL)
    }

    fun close() {
        remote.stop()
        overlay.close()
    }

    private fun open(start: Page) {
        if (overlay.showing) return
        show(start)
    }

    private fun show(start: Page) {
        firstPage = start
        fromSearch = false
        searchQuery = ""
        page = start
        render()
        overlay.open()
    }

    fun back(): Boolean {
        if (!overlay.showing) return false
        if (page != firstPage) {
            val parent = if (fromSearch) Page.SEARCH else firstPage
            fromSearch = false
            navigate(parent)
            overlay.resetSwipe()
        } else {
            close()
        }
        return true
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
        if (theme == "system") updateColors() else refresh()
    }

    private fun updateColors() {
        ui = SettingsUi(view.context, theme)
        refresh()
    }

    private fun navigate(next: Page) {
        if (page == Page.SEARCH && next != Page.SEARCH) {
            fromSearch = next != firstPage
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
        val scan = if (page == Page.HOME) ui.scanButton(labels["scan.scanQr"], ::scan) else null
        panel.addView(ui.page(labels[page.title], labels["settings.back"], { back() }, scan) {
            when (page) {
                Page.HOME -> home(this)
                Page.SEARCH -> search(this)
                Page.REMOTE -> remote.attach(ui, this, localEngine?.openWorkbench)
                Page.NOTIFICATIONS -> general.notifications(ui, this)
                Page.PERMISSIONS -> general.permissions(ui, this)
                Page.DIAGNOSTICS -> general.diagnostics(ui, this, remote.currentName)
                Page.ABOUT -> general.about(ui, this)
                Page.LOCAL -> localEngine?.render(ui, this)
            }
        })
    }

    private fun scan() {
        flow.scan({ notice ->
            if (notice != null) Toast.makeText(activity, notice, Toast.LENGTH_SHORT).show()
        })
    }

    private fun home(body: LinearLayout) = with(ui) {
        body.addView(searchButton(labels["settings.search"]) { navigate(Page.SEARCH) },
            LinearLayout.LayoutParams(-1, -2).apply { bottomMargin = activity.dp(16) })
        body.section {
            addView(row(labels["connection.currentConnection"],
                remote.currentName ?: labels["connection.noConnection"], value = labels["settings.switchHost"],
                icon = R.drawable.ic_settings_host, action = { navigate(Page.REMOTE) }))
            localEngine?.let { addView(row(labels["localAgent.title"], it.summary,
                icon = R.drawable.ic_settings_engine, tone = SettingsUi.Tone.HOSTS, action = ::openLocal)) }
        }
        for ((group, tone, entries) in groups()) {
            body.section(labels[group]) {
                entries.forEach { item -> addView(row(labels[item.title], value = item.value,
                    icon = item.icon, tone = tone, action = item.action)) }
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
        val items = groups().flatMap { it.items } + item(Page.REMOTE, R.drawable.ic_settings_host) +
            listOfNotNull(localEngine?.let { item(Page.LOCAL, R.drawable.ic_settings_engine) })
        fun update(query: String) {
            matches.removeAllViews()
            val found = items.filter { labels[it.title].contains(query.trim(), ignoreCase = true) }
            matches.section {
                if (found.isEmpty()) addView(row(labels["settings.noResults"]))
                found.forEach { item -> addView(row(labels[item.title], icon = item.icon, action = item.action)) }
            }
        }
        input.doAfterTextChanged {
            searchQuery = it.toString()
            update(searchQuery)
        }
        update(searchQuery)
    }

    private fun groups(): List<Group> = buildList {
        add(Group("settings.general", SettingsUi.Tone.GENERAL, listOf(
            Item("settings.language", R.drawable.ic_manage, general.languageName, general::language),
            item(Page.NOTIFICATIONS, R.drawable.ic_settings_bell, general.state(general.notificationsEnabled)),
            item(Page.PERMISSIONS, R.drawable.ic_settings_shield)
        )))
        add(Group("settings.help", SettingsUi.Tone.HELP, listOf(
            item(Page.DIAGNOSTICS, R.drawable.ic_settings_engine),
            item(Page.ABOUT, R.drawable.ic_settings_info, general.version)
        )))
    }

    private fun item(page: Page, icon: Int, value: String? = null) =
        Item(page.title, icon, value) { navigate(page) }

    private data class Item(val title: String, val icon: Int, val value: String? = null, val action: () -> Unit)
    private data class Group(val title: String, val tone: SettingsUi.Tone, val items: List<Item>)

    private enum class Page(val title: String) {
        HOME("settings.title"), SEARCH("settings.search"), REMOTE("settings.switchHost"),
        NOTIFICATIONS("settings.taskNotifications"), PERMISSIONS("settings.permissions"),
        DIAGNOSTICS("settings.diagnostics"), ABOUT("settings.about"), LOCAL("localAgent.title")
    }
}
