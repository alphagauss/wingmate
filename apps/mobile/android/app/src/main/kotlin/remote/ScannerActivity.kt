package dev.wingmate.mobile.remote

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Bundle
import android.provider.Settings
import android.text.InputType
import android.view.Gravity
import android.view.View
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.ContextCompat
import com.google.zxing.BarcodeFormat
import com.journeyapps.barcodescanner.CompoundBarcodeView
import com.journeyapps.barcodescanner.DefaultDecoderFactory
import dev.wingmate.mobile.i18n.ConnectionStrings
import dev.wingmate.mobile.theme.ConnectionUi
import dev.wingmate.mobile.theme.applyWindowInsets
import dev.wingmate.mobile.theme.dp
import dev.wingmate.mobile.theme.rounded

class ScannerActivity : AppCompatActivity() {
    private val labels by lazy { ConnectionStrings(this) }
    private val ui by lazy { ConnectionUi(this) }
    private val prefs by lazy { getSharedPreferences("connection", MODE_PRIVATE) }
    private lateinit var root: LinearLayout
    private var camera: CompoundBarcodeView? = null
    private var manual = false
    private val permission = registerForActivityResult(ActivityResultContracts.RequestPermission()) {
        if (!manual) showScanner()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(ui.background)
            applyWindowInsets()
        }
        setContentView(root)
        if (savedInstanceState?.getBoolean("manual") ?: intent.getBooleanExtra("manual", false)) showAddress()
    }

    override fun onSaveInstanceState(outState: Bundle) {
        outState.putBoolean("manual", manual)
        super.onSaveInstanceState(outState)
    }

    private fun showScanner() {
        manual = false
        setPage("scan.cameraTitle")
        root.addView(ui.button(labels["scan.enterAddress"], action = ::showAddress), -1, -2)
        val granted = ContextCompat.checkSelfPermission(this, Manifest.permission.CAMERA) ==
            PackageManager.PERMISSION_GRANTED
        if (granted) {
            val preview = CompoundBarcodeView(this).apply {
                decoderFactory = DefaultDecoderFactory(listOf(BarcodeFormat.QR_CODE))
                setStatusText(labels["scan.cameraHint"])
            }
            camera = preview
            root.addView(preview, LinearLayout.LayoutParams(-1, 0, 1f))
            preview.decodeContinuous { code ->
                if (isFinishing || camera !== preview) return@decodeContinuous
                val target = ConnectionTarget.parse(code.text.orEmpty())
                if (target == null) preview.setStatusText(labels["scan.invalidQr"]) else finishWith(target)
            }
            preview.resume()
        } else {
            val openSettings = prefs.getBoolean("camera_requested", false) &&
                !shouldShowRequestPermissionRationale(Manifest.permission.CAMERA)
            root.addView(LinearLayout(this).apply {
                orientation = LinearLayout.VERTICAL
                gravity = Gravity.CENTER
                setPadding(dp(24), 0, dp(24), 0)
                addView(ui.text(labels["scan.cameraPermission"], 16f).apply { gravity = Gravity.CENTER })
                val key = if (openSettings) "scan.openSettings" else "scan.allowCamera"
                addView(ui.button(labels[key], primary = true) {
                    if (openSettings) {
                        startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:$packageName")))
                    } else {
                        prefs.edit().putBoolean("camera_requested", true).apply()
                        permission.launch(Manifest.permission.CAMERA)
                    }
                }, -1, -2)
            }, LinearLayout.LayoutParams(-1, 0, 1f))
        }
    }

    private fun showAddress() {
        manual = true
        setPage("scan.addressTitle")
        val field = EditText(this).apply {
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_URI
            imeOptions = EditorInfo.IME_ACTION_GO
            isSingleLine = true
            hint = labels["scan.address"]
            setTextColor(ui.foreground)
            setHintTextColor(ui.muted)
            setPadding(dp(14), 0, dp(14), 0)
            background = rounded(ui.surface, 12, ui.border)
        }
        val submit = {
            val target = ConnectionTarget.parse(field.text.toString(), manual = true)
            if (target == null) field.error = labels["scan.invalidAddress"] else finishWith(target)
        }
        field.setOnEditorActionListener { _, action, _ ->
            if (action != EditorInfo.IME_ACTION_GO) false else { submit(); true }
        }
        root.addView(ScrollView(this).apply {
            addView(LinearLayout(this@ScannerActivity).apply {
                orientation = LinearLayout.VERTICAL
                setPadding(dp(24), dp(12), dp(24), dp(24))
                addView(ui.text(labels["scan.addressHint"], 15f, ui.muted))
                addView(field, LinearLayout.LayoutParams(-1, dp(54)).apply { topMargin = dp(12) })
                addView(ui.button(labels["scan.connect"], primary = true, action = submit),
                    LinearLayout.LayoutParams(-1, -2).apply { topMargin = dp(12) })
                addView(ui.button(labels["scan.scanInstead"]) {
                    getSystemService(InputMethodManager::class.java).hideSoftInputFromWindow(field.windowToken, 0)
                    showScanner()
                })
            })
        }, LinearLayout.LayoutParams(-1, 0, 1f))
        field.requestFocus()
        field.post { getSystemService(InputMethodManager::class.java).showSoftInput(field, InputMethodManager.SHOW_IMPLICIT) }
    }

    private fun setPage(title: String) {
        camera?.pause()
        camera = null
        root.removeAllViews()
        root.addView(LinearLayout(this).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(12), dp(6), dp(12), dp(6))
            addView(ui.button("‹") { finish() }.apply {
                textSize = 26f
                contentDescription = labels["settings.back"]
            }, dp(48), dp(48))
            addView(ui.text(labels[title], 20f, bold = true))
        }, -1, -2)
    }

    private fun finishWith(target: ConnectionTarget) {
        setResult(RESULT_OK, Intent().putExtra("text", target.link))
        finish()
    }

    override fun onResume() {
        super.onResume()
        if (!manual) showScanner()
    }

    override fun onPause() {
        camera?.pause()
        super.onPause()
    }
}
