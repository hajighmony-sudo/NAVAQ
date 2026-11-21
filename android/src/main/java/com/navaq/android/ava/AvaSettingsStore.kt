package com.navaq.android.ava

import android.content.Context
import org.json.JSONObject

enum class AvaMusicSources(val wireValue: String) {
    DEVICE("device"),
    IN_APP("in_app"),
    BOTH("both")
}

enum class AvaAiMode(val wireValue: String) {
    OFFLINE_ONLY("offline_only"),
    ONLINE_ONLY("online_only"),
    ADAPTIVE("adaptive")
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
 * Persisted AVA preferences. The UI can bind its settings controls to this
 * store without coupling AVA to any existing NAVAQ feature.
 */
data class AvaSettings(
    val enabled: Boolean = true,
    val musicSources: AvaMusicSources = AvaMusicSources.BOTH,
    val aiMode: AvaAiMode = AvaAiMode.ADAPTIVE,
    // AVA stays visual-only until the user explicitly selects Voice.
    val responseMode: AvaResponseMode = AvaResponseMode.VISUAL,
    val language: AvaLanguage = AvaLanguage.AUTO,
    // Wake-word activation is an opt-in setting; push-to-talk is the default.
    val activationMode: AvaActivationMode = AvaActivationMode.PUSH_TO_TALK,
    val confirmMusicBeforePlayback: Boolean = false
) {
    fun speechLanguageTag(): String = when (language) {
        AvaLanguage.PERSIAN -> "fa-IR"
        AvaLanguage.ENGLISH -> "en-US"
        AvaLanguage.AUTO -> java.util.Locale.getDefault().toLanguageTag()
    }

    fun preferOfflineSpeech(networkAvailable: Boolean): Boolean = when (aiMode) {
        AvaAiMode.OFFLINE_ONLY -> true
        AvaAiMode.ONLINE_ONLY -> false
        AvaAiMode.ADAPTIVE -> !networkAvailable
    }

    fun toWireJson(): String = JSONObject()
        .put("enabled", enabled)
        .put("music_sources", musicSources.wireValue)
        .put("ai_mode", aiMode.wireValue)
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
        aiMode = enumValue(preferences.getString(KEY_AI_MODE, null), AvaAiMode.ADAPTIVE),
        responseMode = enumValue(preferences.getString(KEY_RESPONSE_MODE, null), AvaResponseMode.VISUAL),
        language = enumValue(preferences.getString(KEY_LANGUAGE, null), AvaLanguage.AUTO),
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
            .putString(KEY_AI_MODE, settings.aiMode.wireValue)
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
                is AvaAiMode -> candidate.wireValue == value
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
        private const val KEY_AI_MODE = "ai_mode"
        private const val KEY_RESPONSE_MODE = "response_mode"
        private const val KEY_LANGUAGE = "language"
        private const val KEY_ACTIVATION_MODE = "activation_mode"
        private const val KEY_CONFIRM_MUSIC = "confirm_music_before_playback"
    }
}
