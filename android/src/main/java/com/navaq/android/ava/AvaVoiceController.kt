package com.navaq.android.ava

import android.content.Context
import androidx.annotation.MainThread

/**
 * Activity-facing AVA voice coordinator.
 *
 * Start listening only from a visible, user-initiated screen after requesting
 * RECORD_AUDIO. It owns one recognizer and forwards the resulting transcript
 * into the existing NAVAQ MediaSession playback service.
 */
class AvaVoiceController(
    context: Context,
    private val onListeningChanged: (Boolean) -> Unit = {},
    private val onError: (Throwable) -> Unit = {}
) {
    private val appContext = context.applicationContext
    private val settingsStore = AvaSettingsStore(appContext)
    private var speechInput: AvaSpeechInput? = null

    @MainThread
    fun startListening() {
        val settings = settingsStore.load()
        check(settings.enabled) { "AVA is disabled in settings" }
        check(AvaPermissions.hasMicrophoneAccess(appContext)) {
            "Microphone permission is required"
        }
        stopListening()

        val input = AvaSpeechInput(
            context = appContext,
            onTranscript = { transcript ->
                try {
                    AvaCommandClient.submitTranscript(appContext, transcript)
                } catch (error: Exception) {
                    onError(error)
                }
            },
            onError = { code ->
                onError(IllegalStateException("Speech recognition failed (code $code)"))
            },
            onListeningChanged = onListeningChanged
        )
        speechInput = input
        try {
            input.startListening(
                languageTag = settings.speechLanguageTag(),
                preferOffline = settings.preferOfflineSpeech()
            )
        } catch (error: Exception) {
            input.release()
            speechInput = null
            onError(error)
        }
    }

    @MainThread
    fun stopListening() {
        speechInput?.release()
        speechInput = null
    }

    @MainThread
    fun release() = stopListening()
}
