package dev.wingmate.mobile

import android.net.Uri
import androidx.appcompat.app.AppCompatActivity
import dev.wingmate.mobile.settings.LocalEngineSettings

@Suppress("UNUSED_PARAMETER")
internal fun localEngineSettings(activity: AppCompatActivity, open: (Uri) -> Unit): LocalEngineSettings? = null

@Suppress("UNUSED_PARAMETER")
internal fun autoStartLocalEngine(activity: AppCompatActivity) = Unit
