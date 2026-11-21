package com.navaq.android.ava

import android.content.Context
import android.speech.tts.TextToSpeech
import java.util.Locale

/**
 * Optional AVA voice output. No speech is produced in Silent or Visual mode.
 * The caller must pass user-visible status/response text and honor the setting.
 */
class AvaSpeechOutput(
    context: Context,
    private val onReady: (Boolean) -> Unit = {}
) : TextToSpeech.OnInitListener {
    private var engine: TextToSpeech? = TextToSpeech(context.applicationContext, this)
    private var ready = false

    override fun onInit(status: Int) {
        ready = status == TextToSpeech.SUCCESS
        onReady(ready)
    }

    fun speak(text: String, settings: AvaSettings): Boolean {
        if (!ready || settings.responseMode != AvaResponseMode.VOICE || text.isBlank()) {
            return false
        }
        val locale = when (settings.language) {
            AvaLanguage.PERSIAN -> Locale.forLanguageTag("fa-IR")
            AvaLanguage.ENGLISH -> Locale.forLanguageTag("en-US")
            AvaLanguage.AUTO -> Locale.getDefault()
        }
        val tts = engine ?: return false
        if (tts.isLanguageAvailable(locale) < TextToSpeech.LANG_AVAILABLE) return false
        tts.language = locale
        return tts.speak(text, TextToSpeech.QUEUE_FLUSH, null, "ava-response") ==
            TextToSpeech.SUCCESS
    }

    fun stop() {
        engine?.stop()
    }

    fun release() {
        engine?.stop()
        engine?.shutdown()
        engine = null
        ready = false
    }
}
