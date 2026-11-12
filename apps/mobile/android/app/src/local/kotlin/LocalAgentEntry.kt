package dev.wingmate.mobile

import android.app.Activity
import android.content.Intent
import dev.wingmate.mobile.web.WorkbenchActivity

internal fun localAgentAction(activity: Activity): (() -> Unit)? = {
    activity.startActivity(Intent(activity, LocalAgentActivity::class.java)
        .putExtra("fromWorkbench", activity is WorkbenchActivity))
}
