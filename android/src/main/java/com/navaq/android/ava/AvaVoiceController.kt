package com.navaq.android.ava

import android.content.Context
import androidx.annotation.MainThread

/**
 * Owns one local speech-recognition session for the visible AVA screen.
 * The Whisper model and microphone pipeline stay alive between utterances.
 */
class AvaVoiceController(
    context: Context,
    private val onListeningChanged: (Boolean) -> Unit = {},
    private val onError: (Throwable) -> Unit = {},
    private val onTranscript: (String) -> Unit = {},
    private val onWakePhraseDetected: () -> Unit = {}
) {
    private val appContext = context.applicationContext
    private val settingsStore = AvaSettingsStore(appContext)
    private var speechInput: AvaSpeechInput? = null

    @Volatile
    private var continuousListening = false

    val isContinuousListening: Boolean
        get() = continuousListening

    @MainThread
    fun startListening() {
        val settings = settingsStore.load()
        check(settings.enabled) { "AVA is disabled in settings" }
        check(AvaPermissions.hasMicrophoneAccess(appContext)) {
            "Microphone permission is required"
        }
        check(AvaSpeechModelManager(appContext).isInstalled()) {
            "Download the offline Persian/English speech model from AVA settings first"
        }

        stopListening()
        continuousListening = true

        val input = AvaSpeechInput(
            context = appContext,
            onTranscript = { transcript ->
                if (continuousListening) {
                    onTranscript(transcript)
                    try {
                        val latestSettings = settingsStore.load()
                        val command = if (latestSettings.activationMode == AvaActivationMode.WAKE_WORD) {
                            extractWakeCommand(transcript)
                        } else {
                            transcript
                        }

                        when {
                            command == null -> Unit // Wake phrase was not spoken; do not execute.
                            command.isBlank() -> onWakePhraseDetected()
                            else -> AvaCommandClient.submitTranscript(appContext, command)
                        }
                    } catch (error: Exception) {
                        onError(error)
                    }
                }
            },
            onError = { code ->
                if (continuousListening) {
                    continuousListening = false
                    onError(IllegalStateException("Offline speech recognition failed (code $code)"))
                }
            },
            onListeningChanged = onListeningChanged
        )
        speechInput = input
        try {
            input.startListening(
                languageTag = settings.speechLanguageTag(),
                preferOffline = true
            )
        } catch (error: Exception) {
            input.release()
            if (speechInput === input) speechInput = null
            continuousListening = false
            onError(error)
        }
    }

    @MainThread
    fun stopListening() {
        continuousListening = false
        speechInput?.release()
        speechInput = null
    }

    @MainThread
    fun release() = stopListening()

    /**
     * Wake phrase detection is deliberately local and scoped to an already
     * user-started, foreground listening session. It is not an always-on
     * background hotword service.
     */
    private fun extractWakeCommand(transcript: String): String? {
        val normalized = transcript
            .lowercase()
            .replace('ي', 'ی')
            .replace('ك', 'ک')
            .trim()
        val patterns = listOf(
            Regex("^(?:هی\\s*)?آوا(?:\\s*[,،:؛.!؟-]?\\s*)(.*)$"),
            Regex("^(?:hey\\s+ava|okay\\s+ava|ok\\s+ava|ava)(?:\\s*[,،:;.!?—-]?\\s*)(.*)$")
        )
        val match = patterns.firstNotNullOfOrNull { it.matchEntire(normalized) } ?: return null
        return match.groupValues.getOrElse(1) { "" }.trim()
    }
}
