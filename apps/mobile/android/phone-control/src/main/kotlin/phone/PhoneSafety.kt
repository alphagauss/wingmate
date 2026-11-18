package dev.wingmate.mobile.runtime.phone

// Foreground protection rules from dsh-Go SafetyGate, ProtectedPages and SensitiveWords (AGPLv3).
internal object PhoneSafety {
    fun blocked(pkg: String) =
        pkg == "dev.wingmate.mobile" ||
            pkg.startsWith("dev.wingmate.mobile.") ||
            pkg in systemPackages ||
            pkg in securityPackages ||
            pkg in settingsPackages ||
            pkg in protectedApps

    private val systemPackages =
        setOf(
            "com.android.settings",
            "com.android.systemui",
            "com.android.permissioncontroller",
            "com.google.android.permissioncontroller",
            "com.android.packageinstaller",
            "com.google.android.packageinstaller",
            "com.miui.packageinstaller",
            "com.android.settings.intelligence",
            "com.miui.securitycenter",
            "com.miui.securityadd",
            "com.miui.securitycore",
            "com.coloros.safecenter",
            "com.oplus.safecenter",
            "com.coloros.securitypermission",
            "com.oplus.securitypermission",
            "com.vivo.permissionmanager",
            "com.iqoo.secure",
            "com.huawei.systemmanager",
            "com.hihonor.systemmanager",
            "com.huawei.security",
            "com.hihonor.security",
            "com.samsung.android.lool",
            "com.samsung.android.sm",
            "com.samsung.accessibility",
            "com.google.android.marvin.talkback",
            "com.android.keychain",
            "com.android.vpndialogs",
            "com.android.certinstaller",
        )

    private val securityPackages =
        setOf(
            "com.android.packageinstaller",
            "com.google.android.packageinstaller",
            "com.miui.packageinstaller",
            "com.samsung.android.packageinstaller",
            "com.oplus.packageinstaller",
            "com.coloros.packageinstaller",
            "com.android.permissioncontroller",
            "com.google.android.permissioncontroller",
            "com.lbe.security.miui",
            "com.miui.securitycenter",
            "com.miui.securityadd",
            "com.coloros.securitypermission",
            "com.oplus.securitypermission",
            "com.coloros.safecenter",
            "com.oplus.safecenter",
            "com.iqoo.secure",
            "com.vivo.permissionmanager",
            "com.vivo.safecenter",
            "com.huawei.systemmanager",
            "com.hihonor.systemmanager",
            "com.huawei.securitymgr",
            "com.android.settings.intelligence",
        )

    private val settingsPackages =
        setOf(
            "com.android.settings",
            "com.miui.settings",
            "com.coloros.settings",
            "com.oplus.settings",
            "com.vivo.settings",
            "com.huawei.settings",
            "com.hihonor.settings",
            "com.samsung.android.settings",
        )

    private val protectedApps =
        setOf(
            // payment
            "com.eg.android.AlipayGphone",
            "com.unionpay",
            "com.chinatelecom.bestpayclient",
            "com.jd.jrapp",
            "cn.gov.pbc.dcep",
            "com.paypal.android.p2pmobile",
            "com.mybank.android.phone",
            "com.webank.wemoney",
            // banks
            "com.icbc",
            "com.chinamworld.main",
            "com.android.bankabc",
            "com.chinamworld.bocmbci",
            "com.bankcomm.Bankcomm",
            "cmb.pb",
            "com.cmbchina.ccd.pluto.cmbActivity",
            "com.yitong.mbank.psbc",
            "cn.com.spdb.mobilebank.per",
            "cn.com.cmbc.newmbank",
            "com.ecitic.bank.mobile",
            "com.cebbank.mobile.cemb",
            "com.pingan.paces.ccms",
            "com.cgbchina.xpt",
            "com.bankofbeijing.mobilebanking",
            // brokers
            "com.hexin.plat.android",
            "com.eastmoney.android.berlin",
            "com.lphtsccft",
            "com.xueqiu.android",
            "cn.futu.trader",
            "com.tigerbrokers.stock",
        )

    private fun wholeWords(word: String): Regex =
        Regex(
            "(?<![\\p{L}\\p{N}_])" +
                word.split(' ').joinToString("\\s+") { Regex.escape(it) } +
                "(?![\\p{L}\\p{N}_])",
            RegexOption.IGNORE_CASE,
        )

    private val otpHint = wholeWords("otp")
    private val oneTimeHint = wholeWords("one-time")

    /** Secret inputs: their contents stay masked even when the person permits typing. */
    val secret: List<String> =
        listOf(
            // l10n:keep external UI matcher
            "支付密码",
            "密码",
            "验证码",
            "校验码",
            "动态码",
            "安全码",
            "cvv",
            "cvc",
            "password",
            "passcode",
            "verification code",
        )

    /** Secret-field view-id words (`et_pwd`, `smsCode`). */
    val secretIdWords: Set<String> =
        setOf(
            "password",
            "passwd",
            "pwd",
            "passcode",
            "otp",
            "cvv",
            "cvc",
            "smscode",
            "verifycode",
            "authcode",
        )

    /** The word that makes this input a secret (password / code) field, or null. */
    fun secretHit(hint: String?, description: String?, viewId: String?): String? {
        for (value in listOf(hint, description)) {
            val text = value?.lowercase() ?: continue
            secret
                .firstOrNull { text.contains(it) }
                ?.let {
                    return it
                }
            // Keep the background reader's OTP / one-time hints without matching "hotpot".
            if (otpHint.containsMatchIn(text)) return "otp"
            if (oneTimeHint.containsMatchIn(text)) return "one-time"
        }
        return idWords(viewId).firstOrNull { it in secretIdWords }
    }

    /**
     * `com.tencent.mm:id/btn_sendMsg` → [btn, send, msg, btnsend, sendmsg]: the id tail split on
     * separators and camelCase, lower-cased, plus each pair of neighbours joined (so `verify_code`
     * also yields `verifycode`).
     */
    fun idWords(viewId: String?): List<String> {
        if (viewId.isNullOrBlank()) return emptyList()
        val tail = viewId.substringAfterLast(":id/").substringAfterLast('/')
        val words = mutableListOf<String>()
        val current = StringBuilder()
        fun flush() {
            if (current.isNotEmpty()) {
                words.add(current.toString().lowercase())
                current.setLength(0)
            }
        }
        for ((index, char) in tail.withIndex()) {
            when {
                !char.isLetterOrDigit() -> flush()
                char.isUpperCase() && index > 0 && tail[index - 1].isLowerCase() -> {
                    flush()
                    current.append(char)
                }
                else -> current.append(char)
            }
        }
        flush()
        return words + words.zipWithNext { a, b -> a + b }
    }
}
