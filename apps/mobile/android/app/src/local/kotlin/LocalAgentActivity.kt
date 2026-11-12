package dev.wingmate.mobile

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.view.View
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.ScrollView
import androidx.activity.enableEdgeToEdge
import androidx.appcompat.app.AlertDialog
import androidx.appcompat.app.AppCompatActivity
import dev.wingmate.mobile.i18n.ConnectionStrings
import dev.wingmate.mobile.runtime.LocalRuntime
import dev.wingmate.mobile.runtime.RuntimeStatus
import dev.wingmate.mobile.theme.ConnectionUi
import dev.wingmate.mobile.theme.applyWindowInsets
import dev.wingmate.mobile.theme.dp
import dev.wingmate.mobile.web.WorkbenchActivity

class LocalAgentActivity : AppCompatActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)

        val labels = ConnectionStrings(this)
        val ui = ConnectionUi(this)
        val status = ui.text("", 16f, ui.muted)
        val progress = ProgressBar(this, null, android.R.attr.progressBarStyleHorizontal)
        val start = ui.button(labels["localAgent.start"], primary = true) { LocalRuntime.start(this) }
        val stop = ui.button(labels["localAgent.stop"]) { LocalRuntime.stop(this) }
        val open = ui.button(labels["localAgent.open"], primary = true) {
            val ready = LocalRuntime.status.value as? RuntimeStatus.Ready ?: return@button
            val link = Uri.parse(ready.address).buildUpon().path("/").appendQueryParameter("token", ready.token)
            val workbench = Intent(this, WorkbenchActivity::class.java)
                .putExtra("link", link.toString()).putExtra("localAgent", true)
            if (intent.getBooleanExtra("fromWorkbench", false)) {
                workbench.addFlags(Intent.FLAG_ACTIVITY_REORDER_TO_FRONT)
            }
            startActivity(workbench)
            finish()
        }
        val logs = ui.button(labels["localAgent.logs"]) {
            val content = ui.text(LocalRuntime.logs(), 13f).apply {
                setPadding(dp(20), dp(10), dp(20), dp(10))
                setTextIsSelectable(true)
            }
            AlertDialog.Builder(this).setTitle(labels["localAgent.logs"])
                .setView(ScrollView(this).apply { addView(content) })
                .setPositiveButton(android.R.string.ok, null).show()
        }
        val body = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(24), dp(20), dp(24), dp(20))
            addView(ui.text(labels["localAgent.title"], 26f, bold = true))
            addView(status, LinearLayout.LayoutParams(-1, -2).apply {
                topMargin = dp(12)
                bottomMargin = dp(16)
            })
            addView(progress)
            addView(start)
            addView(open)
            addView(stop)
            addView(logs)
        }
        setContentView(FrameLayout(this).apply {
            setBackgroundColor(ui.background)
            addView(ScrollView(this@LocalAgentActivity).apply { addView(body) })
            applyWindowInsets()
        })
        LocalRuntime.status.observe(this) { value ->
            status.text = when (value) {
                RuntimeStatus.Stopped -> labels["localAgent.stopped"]
                is RuntimeStatus.Installing -> labels["localAgent.installing"]
                RuntimeStatus.Starting -> labels["localAgent.starting"]
                is RuntimeStatus.Ready -> labels["localAgent.running"]
                RuntimeStatus.Stopping -> labels["localAgent.stopping"]
                is RuntimeStatus.Failed -> value.reason
            }
            val installation = value as? RuntimeStatus.Installing
            progress.visibility = if (installation != null) View.VISIBLE else View.GONE
            if (installation != null) {
                progress.isIndeterminate = installation.progress == 0
                progress.progress = installation.progress
            }
            start.visibility = if (value is RuntimeStatus.Failed || value == RuntimeStatus.Stopped) View.VISIBLE else View.GONE
            open.visibility = if (value is RuntimeStatus.Ready) View.VISIBLE else View.GONE
            stop.visibility = if (value is RuntimeStatus.Ready || value == RuntimeStatus.Starting) View.VISIBLE else View.GONE
        }
    }
}
