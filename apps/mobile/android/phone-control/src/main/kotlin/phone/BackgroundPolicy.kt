package dev.wingmate.mobile.runtime.phone

// The display flags used by dsh-Go's background screen require Android 14 or later.
internal object BackgroundPolicy {
    const val REQUIREMENT = "The independent background screen requires Android 14 or later."

    fun supported(sdk: Int) = sdk >= 34
}
