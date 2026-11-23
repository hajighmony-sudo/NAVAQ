package com.navaq.android.ava

import android.content.Context
import org.json.JSONObject

enum class AvaMusicSources(val wireValue: String) {
    DEVICE("device"),
    IN_APP("in_app"),
    BOTH("both")
}

enum class AvaResponseMode(val wireValue: String) {
    SILENT("silent"),
    VISUAL("visual"),
    VOICE("voice")
}

enum class AvaLanguage(val wireValue: String) {
    AUTO("auto"),
    PERSIAN("persian"),
    ENGLISH("english")
}

enum class AvaActivationMode(val wireValue: String) {
    PUSH_TO_TALK("push_to_talk"),
    WAKE_WORD("wake_word")
}

/**
 * Persisted AVA preferences. Persian is the first-run language; users can
 * explicitly switch to English or automatic Persian/English detection.
 */
data class AvaSettings(
    val enabled: Boolean = true,
    val musicSources: AvaMusicSources = AvaMusicSources.BOTH,
    // AVA stays visual-only until the user explicitly selects Voice.
    val responseMode: AvaResponseMode = AvaResponseMode.VISUAL,
    val language: AvaLanguage = AvaLanguage.PERSIAN,
    // Wake-word activation is an opt-in setting; push-to-talk is the default.
    val activationMode: AvaActivationMode = AvaActivationMode.PUSH_TO_TALK,
    val confirmMusicBeforePlayback: Boolean = false
) {
    fun speechLanguageTag(): String = when (language) {
        AvaLanguage.PERSIAN -> "fa-IR"
        AvaLanguage.ENGLISH -> "en-US"
        // Keep automatic mode deterministic and bilingual rather than relying
        // on the phone's system locale, which may be unrelated to the speaker.
        AvaLanguage.AUTO -> ""
    }

    fun preferOfflineSpeech(): Boolean = true

    fun toWireJson(): String = JSONObject()
        .put("enabled", enabled)
        .put("music_sources", musicSources.wireValue)
        .put("response_mode", responseMode.wireValue)
        .put("language", language.wireValue)
        .put("activation_mode", activationMode.wireValue)
        .put("confirm_music_before_playback", confirmMusicBeforePlayback)
        .toString()
}

class AvaSettingsStore(context: Context) {
    private val preferences = context.applicationContext.getSharedPreferences(
        PREFS_NAME,
        Context.MODE_PRIVATE
    )

    fun load(): AvaSettings = AvaSettings(
        enabled = preferences.getBoolean(KEY_ENABLED, true),
        musicSources = enumValue(preferences.getString(KEY_MUSIC_SOURCES, null), AvaMusicSources.BOTH),
        responseMode = enumValue(preferences.getString(KEY_RESPONSE_MODE, null), AvaResponseMode.VISUAL),
        language = enumValue(preferences.getString(KEY_LANGUAGE, null), AvaLanguage.PERSIAN),
        activationMode = enumValue(
            preferences.getString(KEY_ACTIVATION_MODE, null),
            AvaActivationMode.PUSH_TO_TALK
        ),
        confirmMusicBeforePlayback = preferences.getBoolean(KEY_CONFIRM_MUSIC, false)
    )

    fun save(settings: AvaSettings) {
        preferences.edit()
            .putBoolean(KEY_ENABLED, settings.enabled)
            .putString(KEY_MUSIC_SOURCES, settings.musicSources.wireValue)
            .putString(KEY_RESPONSE_MODE, settings.responseMode.wireValue)
            .putString(KEY_LANGUAGE, settings.language.wireValue)
            .putString(KEY_ACTIVATION_MODE, settings.activationMode.wireValue)
            .putBoolean(KEY_CONFIRM_MUSIC, settings.confirmMusicBeforePlayback)
            .apply()
    }

    private inline fun <reified T> enumValue(value: String?, fallback: T): T where T : Enum<T> {
        return enumValues<T>().firstOrNull { candidate ->
            when (candidate) {
                is AvaMusicSources -> candidate.wireValue == value
                is AvaResponseMode -> candidate.wireValue == value
                is AvaLanguage -> candidate.wireValue == value
                is AvaActivationMode -> candidate.wireValue == value
                else -> candidate.name.equals(value, ignoreCase = true)
            }
        } ?: fallback
    }

    companion object {
        private const val PREFS_NAME = "navaq_ava_preferences"
        private const val KEY_ENABLED = "enabled"
        private const val KEY_MUSIC_SOURCES = "music_sources"
        private const val KEY_RESPONSE_MODE = "response_mode"
        private const val KEY_LANGUAGE = "language"
        private const val KEY_ACTIVATION_MODE = "activation_mode"
        private const val KEY_CONFIRM_MUSIC = "confirm_music_before_playback"
    }
}
