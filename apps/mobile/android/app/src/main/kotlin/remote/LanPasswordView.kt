package dev.wingmate.mobile.remote

import android.content.Context
import android.content.res.ColorStateList
import android.graphics.Rect
import android.graphics.Typeface
import android.text.InputType
import android.text.method.PasswordTransformationMethod
import android.view.Gravity
import android.view.KeyEvent
import android.view.View
import android.view.inputmethod.EditorInfo
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.ImageButton
import android.widget.LinearLayout
import dev.wingmate.mobile.R
import dev.wingmate.mobile.i18n.ConnectionStrings
import dev.wingmate.mobile.theme.ConnectionUi
import dev.wingmate.mobile.theme.dp

internal fun lanPasswordView(
    context: Context,
    ui: ConnectionUi,
    submit: (String, (String) -> Unit) -> Unit,
): View =
    LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(context.dp(8), 0, context.dp(8), context.dp(8))
        val labels = ConnectionStrings(context)
        val input =
            EditText(context).apply {
                id = View.generateViewId()
                setSingleLine(true)
                inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
                typeface = Typeface.DEFAULT
                imeOptions =
                    EditorInfo.IME_ACTION_GO or EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING
                setTextColor(ui.foreground)
                setPaddingRelative(context.dp(10), 0, context.dp(48), 0)
                textSize = 16f
                background = ui.modelInputBackground()
            }
        addView(ui.text(labels["connection.lanPassword"], 14f).apply { labelFor = input.id })
        addView(
            FrameLayout(context).apply {
                addView(input, FrameLayout.LayoutParams(-1, context.dp(44)))
                addView(
                    ImageButton(context).apply {
                        setImageResource(R.drawable.ic_eye)
                        background = null
                        imageTintList = ColorStateList.valueOf(ui.muted)
                        contentDescription = labels["connection.showPassword"]
                        setOnClickListener {
                            val visible = input.transformationMethod is PasswordTransformationMethod
                            input.transformationMethod =
                                if (visible) null else PasswordTransformationMethod.getInstance()
                            input.setSelection(input.text.length)
                            setImageResource(
                                if (visible) R.drawable.ic_eye_off else R.drawable.ic_eye,
                            )
                            contentDescription =
                                labels[
                                    if (visible) "connection.hidePassword"
                                    else "connection.showPassword"]
                        }
                    },
                    FrameLayout.LayoutParams(context.dp(44), context.dp(44), Gravity.END),
                )
            },
            LinearLayout.LayoutParams(-1, -2).apply { topMargin = context.dp(6) },
        )
        val error =
            ui.text("", 14f, ui.danger).apply {
                visibility = View.GONE
                accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE
            }
        addView(error)
        val connect = ui.button(labels["scan.connect"], primary = true) {}
        fun send() {
            if (!connect.isEnabled || input.text.isEmpty()) return
            connect.isEnabled = false
            error.visibility = View.GONE
            submit(input.text.toString()) { message ->
                connect.isEnabled = true
                error.text = message
                error.visibility = View.VISIBLE
            }
        }
        connect.setOnClickListener { send() }
        input.setOnEditorActionListener { _, action, event ->
            if (
                action == EditorInfo.IME_ACTION_GO ||
                    (event?.keyCode == KeyEvent.KEYCODE_ENTER &&
                        event.action == KeyEvent.ACTION_DOWN)
            ) {
                send()
                true
            } else false
        }
        input.setOnFocusChangeListener { _, focused ->
            if (focused) post { requestRectangleOnScreen(Rect(0, 0, width, height)) }
        }
        addOnLayoutChangeListener { _, _, _, _, _, _, _, _, _ ->
            if (input.hasFocus()) post { requestRectangleOnScreen(Rect(0, 0, width, height)) }
        }
        addView(connect, LinearLayout.LayoutParams(-1, -2).apply { topMargin = context.dp(8) })
    }
