package dev.wingmate.mobile.remote

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.net.Uri
import android.os.Bundle
import android.provider.Settings
import android.text.InputType
import android.view.Gravity
import android.view.View
import android.view.WindowManager
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import androidx.core.content.ContextCompat
import androidx.core.widget.doAfterTextChanged
import com.google.zxing.BarcodeFormat
import com.journeyapps.barcodescanner.BarcodeView
import com.journeyapps.barcodescanner.DefaultDecoderFactory
import dev.wingmate.mobile.i18n.ConnectionStrings
import dev.wingmate.mobile.theme.ConnectionUi
import dev.wingmate.mobile.theme.applyWindowInsets
import dev.wingmate.mobile.theme.dp
import dev.wingmate.mobile.theme.rounded

// Layout and camera/manual switch ported from apps/native/src/app/scan.tsx.
class ScannerActivity : AppCompatActivity() {
    private val labels by lazy { ConnectionStrings(this) }
    private lateinit var camera: BarcodeView
    private lateinit var title: TextView
    private lateinit var preview: FrameLayout
    private lateinit var permissionPanel: LinearLayout
    private lateinit var allowCamera: androidx.appcompat.widget.AppCompatButton
    private lateinit var cameraActions: LinearLayout
    private lateinit var manualPanel: LinearLayout
    private lateinit var address: EditText
    private lateinit var error: TextView
    private lateinit var retry: View
    private var manual = false
    private var requested = false
    private var accepted = false
    private val permission = registerForActivityResult(ActivityResultContracts.RequestPermission()) {
        updateCamera()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        requested = getSharedPreferences("connection", MODE_PRIVATE).getBoolean("camera_requested", false)
        window.setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE)
        val ui = ConnectionUi(this)
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(ui.background)
            applyWindowInsets()
        }
        title = ui.text(labels["scan.cameraTitle"], 18f, bold = true)
        root.addView(LinearLayout(this).apply {
            gravity = Gravity.CENTER_VERTICAL
            setPadding(dp(20), dp(8), dp(12), dp(12))
            addView(title, LinearLayout.LayoutParams(0, -2, 1f))
            addView(ui.button("×") { finish() }.apply {
                textSize = 26f
                contentDescription = labels["scan.closeScanner"]
            }, dp(48), dp(48))
        })
        camera = BarcodeView(this).apply {
            decoderFactory = DefaultDecoderFactory(listOf(BarcodeFormat.QR_CODE))
        }
        preview = FrameLayout(this).apply {
            background = rounded(Color.BLACK, 24)
            clipToOutline = true
            addView(camera, FrameLayout.LayoutParams(-1, -1))
            addView(View(this@ScannerActivity).apply {
                background = rounded(Color.TRANSPARENT, 24, Color.parseColor("#ccffffff"))
            }, FrameLayout.LayoutParams(dp(256), dp(256), Gravity.CENTER))
        }
        root.addView(preview, LinearLayout.LayoutParams(-1, 0, 1f).apply {
            setMargins(dp(20), 0, dp(20), 0)
        })
        permissionPanel = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER
            setPadding(dp(32), 0, dp(32), 0)
            addView(ui.text(labels["scan.cameraPermission"], 16f).apply { gravity = Gravity.CENTER })
            allowCamera = ui.button(labels["scan.allowCamera"], primary = true) {
                if (requested && !shouldShowRequestPermissionRationale(Manifest.permission.CAMERA)) {
                    startActivity(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.parse("package:$packageName")))
                } else {
                    requested = true
                    getSharedPreferences("connection", MODE_PRIVATE).edit().putBoolean("camera_requested", true).apply()
                    permission.launch(Manifest.permission.CAMERA)
                }
            }
            addView(allowCamera, LinearLayout.LayoutParams(-2, -2).apply { topMargin = dp(24) })
            addView(ui.button(labels["scan.enterAddress"]) { switchMode(true) })
        }
        root.addView(permissionPanel, LinearLayout.LayoutParams(-1, 0, 1f))
        error = ui.text(labels["scan.invalidQr"], 14f, ui.danger).apply {
            gravity = Gravity.CENTER
            visibility = View.GONE
        }
        retry = ui.button(labels["scan.qrRetry"]) { armCamera() }.apply {
            background = rounded(Color.TRANSPARENT, 16, ui.border)
            visibility = View.GONE
        }
        cameraActions = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(24), dp(24), dp(24), dp(24))
            addView(ui.text(labels["scan.cameraHint"], 14f, ui.muted).apply { gravity = Gravity.CENTER })
            addView(error, LinearLayout.LayoutParams(-1, -2).apply { topMargin = dp(12) })
            addView(retry, LinearLayout.LayoutParams(-1, -2).apply { topMargin = dp(12) })
            addView(ui.button(labels["scan.enterAddress"]) { switchMode(true) }, LinearLayout.LayoutParams(-1, -2).apply {
                topMargin = dp(12)
            })
        }
        root.addView(cameraActions)
        address = EditText(this).apply {
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_URI
            imeOptions = EditorInfo.IME_ACTION_GO
            isSingleLine = true
            textSize = 16f
            contentDescription = labels["scan.address"]
            setTextColor(ui.foreground)
            background = rounded(ui.surface, 12, ui.border)
            setPadding(dp(14), 0, dp(14), 0)
            setOnEditorActionListener { _, id, _ ->
                if (id == EditorInfo.IME_ACTION_GO) { submitAddress(); true } else false
            }
        }
        val connect = ui.button(labels["scan.connect"], primary = true, action = ::submitAddress).apply {
            isEnabled = false
            alpha = .5f
        }
        address.doAfterTextChanged {
            address.error = null
            connect.isEnabled = !it.isNullOrBlank()
            connect.alpha = if (connect.isEnabled) 1f else .5f
        }
        manualPanel = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(20), dp(8), dp(20), 0)
            addView(ui.text(labels["scan.addressHint"], 14f, ui.muted))
            addView(address, LinearLayout.LayoutParams(-1, dp(48)).apply { topMargin = dp(16) })
            addView(connect, LinearLayout.LayoutParams(-1, -2).apply { topMargin = dp(16) })
            addView(ui.button(labels["scan.scanInstead"]) { switchMode(false) }, LinearLayout.LayoutParams(-1, -2).apply {
                topMargin = dp(16)
            })
        }
        root.addView(manualPanel, LinearLayout.LayoutParams(-1, 0, 1f))
        setContentView(root)
        armCamera()
        if (intent.getBooleanExtra("manual", false)) switchMode(true)
    }

    override fun onResume() {
        super.onResume()
        updateCamera()
    }

    private fun updateCamera() {
        val granted = ContextCompat.checkSelfPermission(this, Manifest.permission.CAMERA) ==
            PackageManager.PERMISSION_GRANTED
        preview.visibility = if (granted && !manual) View.VISIBLE else View.GONE
        cameraActions.visibility = preview.visibility
        permissionPanel.visibility = if (!granted && !manual) View.VISIBLE else View.GONE
        allowCamera.text = labels[if (!granted && requested &&
            !shouldShowRequestPermissionRationale(Manifest.permission.CAMERA)) "scan.openSettings" else "scan.allowCamera"]
        manualPanel.visibility = if (manual) View.VISIBLE else View.GONE
        if (preview.visibility == View.VISIBLE && !accepted) camera.resume() else camera.pause()
    }

    private fun armCamera() {
        error.visibility = View.GONE
        retry.visibility = View.GONE
        camera.decodeSingle { result ->
            val target = ConnectionTarget.parse(result.text.orEmpty())
            if (target != null) finishWith(target) else {
                error.visibility = View.VISIBLE
                retry.visibility = View.VISIBLE
            }
        }
    }

    private fun switchMode(enterAddress: Boolean) {
        manual = enterAddress
        title.text = if (manual) labels["scan.addressTitle"] else labels["scan.cameraTitle"]
        val keyboard = getSystemService(InputMethodManager::class.java)
        if (!manual) {
            keyboard.hideSoftInputFromWindow(address.windowToken, 0)
            armCamera()
        }
        updateCamera()
        if (manual) {
            address.requestFocus()
            address.post { keyboard.showSoftInput(address, InputMethodManager.SHOW_IMPLICIT) }
        }
    }

    private fun submitAddress() {
        val target = ConnectionTarget.parse(address.text.toString(), manual = true)
        if (target == null) {
            address.error = labels["scan.invalidAddress"]
            return
        }
        finishWith(target)
    }

    private fun finishWith(target: ConnectionTarget) {
        if (accepted) return
        accepted = true
        setResult(RESULT_OK, Intent().putExtra("text", target.link))
        finish()
    }

    override fun onPause() {
        camera.pause()
        super.onPause()
    }
}
