package dev.wingmate.mobile.runtime.phone

import android.content.Context
import android.os.Handler
import android.os.Looper
import androidx.lifecycle.LiveData
import androidx.lifecycle.MutableLiveData
import dev.wingmate.mobile.phone.R
import dev.wingmate.mobile.runtime.phone.McpServer.Output
import java.util.concurrent.atomic.AtomicBoolean

/**
 * The phone owns authorization and its foreground execution state. All device actions run on main.
 */
class PhoneControl private constructor(context: Context) {
    private val preferences = context.getSharedPreferences("phone_control", Context.MODE_PRIVATE)
    private val main = Handler(Looper.getMainLooper())
    private val connection = MutableLiveData(false)
    val connected: LiveData<Boolean> = connection
    private val background = BackgroundPhone(context)

    val backgroundStatus: BackgroundPhoneStatus
        get() = background.status

    val backgroundChanged: LiveData<Unit>
        get() = background.changed

    private class SwitchState(val enabled: Boolean)

    @Volatile private var state = SwitchState(preferences.getBoolean("enabled", false))
    val enabled: Boolean
        get() = state.enabled

    fun interface ToolsListener {
        fun toolsChanged()
    }

    class Request(
        val call: McpServer.Call,
        internal val prepareAction: ((Context) -> Action)? = null,
        internal val reply: (Output) -> Unit,
    ) {
        internal val cancelled = AtomicBoolean(false)
    }

    class Action(
        val summary: String,
        internal val output: Output? = null,
        internal val perform: (live: () -> Boolean, complete: (Result<Boolean>) -> Unit) -> Unit,
    ) {
        internal constructor(
            summary: String,
            perform: () -> Boolean,
        ) : this(summary, perform = { _, complete -> complete(runCatching(perform)) })
    }

    @Volatile private var listener: ToolsListener? = null
    private var service: PhoneAccessibility? = null
    private var observation: PhoneAccessibility.Observation? = null
    private var backgroundObservation: PhoneAccessibility.Observation? = null
    private var pending: Pending? = null
    @Volatile private var running: Request? = null

    private class Pending(
        val request: Request,
        val service: PhoneAccessibility,
        val action: Action,
        val dismiss: () -> Unit,
        val timeout: Runnable,
    )

    fun attach(next: ToolsListener) {
        listener = next
    }

    fun detach(current: ToolsListener, reason: String) {
        if (listener === current) listener = null
        main.post { retire(reason) }
    }

    fun setEnabled(value: Boolean) {
        if (enabled == value) return
        state = SwitchState(value)
        preferences.edit().putBoolean("enabled", value).apply()
        if (!value) retire("Phone control was turned off.")
        listener?.toolsChanged()
    }

    internal fun connect(next: PhoneAccessibility) {
        retire("The Android accessibility service reconnected.")
        service = next
        connection.value = true
    }

    internal fun disconnect(current: PhoneAccessibility) {
        if (service !== current) return
        retire("The Android accessibility service disconnected.")
        service = null
        connection.value = false
    }

    internal fun invalidate(current: PhoneAccessibility) {
        if (service === current) retire("The phone screen turned off.")
    }

    private fun retire(reason: String) {
        observation = null
        backgroundObservation = null
        pending?.let { finish(it, Output(reason, true)) }
        running?.let { finishRunning(it, Output(reason, true)) }
        background.close()
    }

    fun requestBackgroundPermission() = background.requestPermission()

    fun stopBackground() {
        backgroundObservation = null
        val stopped = Output("The background screen was closed.", true)
        pending?.takeIf { it.request.call.background }?.let { finish(it, stopped) }
        running?.takeIf { it.call.background }?.let { finishRunning(it, stopped) }
        background.close()
    }

    fun preview(): Result<String> = runCatching {
        requireService().read(detail = false, preview = true).text
    }

    fun call(request: Request) {
        val admission = state
        main.post {
            if (request.cancelled.get()) return@post
            val call = request.call
            val response = runCatching {
                if (call.background) phoneCheck(background.supported, BackgroundPolicy.REQUIREMENT)
                phoneCheck(
                    admission.enabled && state === admission,
                    "Phone control was disabled. Check Wingmate settings on the phone.",
                )
                phoneCheck(
                    pending == null && running == null,
                    "A phone request is already in progress.",
                )
                val driver = requireService()
                when (call.tool) {
                    "apps" -> Output(driver.apps(call.args["query"] as? String ?: ""))
                    "screen" ->
                        readScreen(
                            request,
                            driver,
                            if (call.background) background.target() else null,
                        )
                    else -> {
                        val action =
                            request.prepareAction?.invoke(driver)
                                ?: if (call.background) backgroundAction(call, driver)
                                else driver.prepare(call, observation)
                        confirm(request, driver, action)
                        null
                    }
                }
            }
                .getOrElse(::failure)
            if (response != null && !request.cancelled.get()) request.reply(response)
        }
    }

    private fun readScreen(
        request: Request,
        driver: PhoneAccessibility,
        target: BackgroundPhone.Target?,
    ): Output? {
        fun remember(value: PhoneAccessibility.Observation?) {
            if (target == null) observation = value else backgroundObservation = value
        }
        remember(null)
        val observed = driver.read(request.call.args["detail"] == true, background = target)
        if (request.call.args["image"] != true) {
            remember(observed)
            return Output(observed.text)
        }
        running = request
        driver.screenshot(observed) { result ->
            if (!isRunning(request)) return@screenshot
            if (target != null && !background.matches(target)) {
                finishRunning(
                    request,
                    Output("The background screen was closed or lost authorization.", true),
                )
                background.close()
                return@screenshot
            }
            val frame = result.getOrNull()
            remember(observed.copy(frame = frame))
            val note =
                if (frame != null) "Screenshot size: ${frame.width}×${frame.height} pixels."
                else "Screenshot unavailable. " + failure(result.exceptionOrNull()!!).text
            finishRunning(request, Output("${observed.text}\n$note", jpeg = frame?.jpeg))
        }
        return null
    }

    private fun backgroundAction(
        call: McpServer.Call,
        driver: PhoneAccessibility,
    ): Action {
        if (call.tool == "open") {
            background.requireCanOpen()
            val app = driver.launchable(call.args["app"] as String)
            val summary = driver.getString(R.string.phone_background_open, app.label)
            return Action(summary) { live, complete ->
                background.open(app.component, live, complete)
            }
        }
        val target = background.target()
        val action = driver.prepare(call, backgroundObservation, target)
        val summary = driver.getString(R.string.phone_background_action, action.summary)
        return Action(summary) { live, complete ->
            val authorized = {
                if (!live()) false
                else if (background.matches(target)) true
                else {
                    complete(
                        Result.failure(
                            PhoneProblem("The background screen was closed or lost authorization."),
                        ),
                    )
                    background.close()
                    false
                }
            }
            if (authorized()) action.perform(authorized, complete)
        }
    }

    private fun requireService(): PhoneAccessibility {
        val driver =
            service ?: throw PhoneProblem("Enable Wingmate's accessibility service on the phone.")
        phoneCheck(driver.ready(), "Unlock the phone and keep its screen on.")
        return driver
    }

    private fun confirm(
        request: Request,
        driver: PhoneAccessibility,
        action: Action,
    ) {
        if (request.cancelled.get()) return
        val close =
            PhoneConfirmation.show(driver, action.summary) { allow ->
                val current = pending?.takeIf { it.request === request } ?: return@show
                if (allow) approve(current)
                else finish(current, Output("The user cancelled the phone action.", true))
            }
        val timeout = Runnable {
            val current = pending?.takeIf { it.request === request } ?: return@Runnable
            finish(current, Output("Phone confirmation expired.", true))
        }
        pending = Pending(request, driver, action, close, timeout)
        main.postDelayed(timeout, 30_000)
    }

    private fun approve(current: Pending) {
        clear(current)
        val call = current.request.call
        forgetObservation(call)
        if (current.request.cancelled.get()) return
        running = current.request
        runCatching {
            phoneCheck(
                enabled && requireService() === current.service,
                "Phone control is no longer available.",
            )
            current.action.perform({ isRunning(current.request) }) { result ->
                if (!isRunning(current.request)) return@perform
                val output =
                    result
                        .mapCatching { accepted ->
                            phoneCheck(
                                accepted,
                                "Android declined the action. Read the screen again for its current state.",
                            )
                            current.action.output
                                ?: when {
                                    call.tool == "open" && call.background ->
                                        Output(
                                            "Opened the app on the independent background screen. Read it with screen(background: true).",
                                        )
                                    call.tool == "open" ->
                                        Output(
                                            "Android received the launch request. If Android asks to allow opening the app, confirm on the phone before reading the screen.",
                                        )
                                    call.background ->
                                        Output(
                                            "Android accepted the background action. Read it again with screen(background: true) to verify the result.",
                                        )
                                    else ->
                                        Output(
                                            "Android accepted the action. Read the screen again to verify the result.",
                                        )
                                }
                        }
                        .getOrElse(::failure)
                finishRunning(current.request, output)
            }
        }
            .onFailure { finishRunning(current.request, failure(it)) }
    }

    private fun isRunning(request: Request) =
        running === request && enabled && !request.cancelled.get()

    private fun finishRunning(request: Request, result: Output) {
        if (running !== request) return
        running = null
        if (!request.cancelled.get()) request.reply(result)
    }

    fun cancel(request: Request) {
        request.cancelled.set(true)
        main.post {
            if (running === request) {
                running = null
                forgetObservation(request.call)
                if (request.call.background && request.call.tool == "open") background.close()
            }
            pending
                ?.takeIf { it.request === request }
                ?.let {
                    forgetObservation(request.call)
                    clear(it)
                }
        }
    }

    private fun forgetObservation(call: McpServer.Call) {
        if (call.background) backgroundObservation = null else observation = null
    }

    private fun clear(current: Pending) {
        pending = null
        main.removeCallbacks(current.timeout)
        current.dismiss()
    }

    private fun finish(current: Pending, response: Output) {
        clear(current)
        if (!current.request.cancelled.get()) current.request.reply(response)
    }

    private fun failure(error: Throwable) =
        Output(
            if (error is PhoneProblem) error.message.orEmpty()
            else
                "The phone could not complete this request. Check the current screen and Wingmate settings.",
            true,
        )

    companion object {
        @Volatile private var instance: PhoneControl? = null

        fun get(context: Context): PhoneControl =
            instance
                ?: synchronized(this) {
                    instance ?: PhoneControl(context.applicationContext).also { instance = it }
                }
    }
}
