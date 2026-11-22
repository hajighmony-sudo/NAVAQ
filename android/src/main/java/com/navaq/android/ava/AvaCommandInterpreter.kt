package com.navaq.android.ava

import android.content.Context
import com.navaq.android.core.NavaqNativeBridge
import org.json.JSONObject
import java.util.Locale

/**
 * Thin Android adapter around the Rust AVA intent engine.
 * Android-owned actions are mapped here, while music/playback decisions remain
 * in Rust. System actions are deliberately limited to safe, user-visible intents.
 */
class AvaCommandInterpreter(context: Context) {
    private val appContext = context.applicationContext
    private val settingsStore = AvaSettingsStore(appContext)

    fun interpret(transcript: String): JSONObject {
        val settings = settingsStore.load()
        if (!settings.enabled) {
            return JSONObject().put("error", "ava_disabled")
        }

        val command = transcript.trim().lowercase(Locale.ROOT)
            .replace('ي', 'ی')
            .replace('ك', 'ک')
            .replace('‌', ' ')
            .replace(Regex("[،,.!?؟؛:،]"), " ")
            .replace(Regex("\\s+"), " ")
            .trim()
        val asciiDigits = command.map { char ->
            when (char) {
                '۰' -> '0'; '۱' -> '1'; '۲' -> '2'; '۳' -> '3'; '۴' -> '4'
                '۵' -> '5'; '۶' -> '6'; '۷' -> '7'; '۸' -> '8'; '۹' -> '9'
                '٠' -> '0'; '١' -> '1'; '٢' -> '2'; '٣' -> '3'; '٤' -> '4'
                '٥' -> '5'; '٦' -> '6'; '٧' -> '7'; '٨' -> '8'; '٩' -> '9'
                else -> char
            }
        }.joinToString("")
        val dialMatch = listOf(
            Regex("""^(?:call|dial)\s+([+0-9][0-9+ ()-]{4,22})$""", RegexOption.IGNORE_CASE),
            Regex("""^(?:به شماره\s*)?([+0-9][0-9+ ()-]{4,22})\s*(?:زنگ بزن|تماس بگیر|شماره گیری کن)$"""),
            Regex("""^(?:زنگ بزن به|تماس بگیر با)\s*([+0-9][0-9+ ()-]{4,22})$""")
        ).firstNotNullOfOrNull { it.find(asciiDigits) }
        val dialNumber = dialMatch?.groupValues?.getOrNull(1)
            ?.replace(Regex("[^0-9+]"), "")
            ?.takeIf { number ->
                number.length in 5..18 &&
                    number.count { it == '+' } <= 1 &&
                    (!number.contains('+') || number.startsWith('+'))
            }
        if (dialNumber != null) {
            return JSONObject()
                .put("route", "local")
                .put("intent", JSONObject()
                    .put("type", "android_action")
                    .put("action", "open_dialer")
                    .put("phone_number", dialNumber))
        }

        val systemAction = when {
            command in setOf(
                "تنظیمات اعلان ها را باز کن", "تنظیمات اعلان را باز کن",
                "تنظیمات نوتیفیکیشن را باز کن", "open notification settings"
            ) -> "open_notification_settings"
            command in setOf(
                "وای فای را باز کن", "وایفای را باز کن", "تنظیمات وای فای",
                "تنظیمات wifi", "open wifi settings", "open wi fi settings"
            ) -> "open_wifi_settings"
            command in setOf(
                "بلوتوث را باز کن", "تنظیمات بلوتوث", "open bluetooth settings"
            ) -> "open_bluetooth_settings"
            command in setOf(
                "تنظیمات باتری را باز کن", "تنظیمات باتری", "open battery settings"
            ) -> "open_battery_settings"
            command in setOf(
                "پیام ها را باز کن", "پیامک ها را باز کن", "پیام رسان را باز کن",
                "open messages", "open messaging"
            ) -> "open_messages"
            command in setOf(
                "دوربین را باز کن", "دوربین رو باز کن", "open camera"
            ) -> "open_camera"
            command in setOf(
                "آلارم را باز کن", "زنگ هشدار را باز کن", "ساعت و آلارم را باز کن",
                "open alarms", "open clock"
            ) -> "open_clock"
            else -> null
        }
        if (systemAction != null) {
            return JSONObject()
                .put("route", "local")
                .put("intent", JSONObject()
                    .put("type", "android_action")
                    .put("action", systemAction))
        }

        val result = NavaqNativeBridge.nativeInterpretAva(
            transcript,
            settings.toWireJson(),
            false
        )
        return JSONObject(result)
    }

    fun currentSettings(): AvaSettings = settingsStore.load()

    fun saveSettings(settings: AvaSettings) = settingsStore.save(settings)
}
