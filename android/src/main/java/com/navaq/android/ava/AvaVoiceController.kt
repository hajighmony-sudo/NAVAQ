package com.navaq.android.ava

import android.content.Context
import android.os.Handler
import android.os.Looper
import androidx.annotation.MainThread

/**
 * Activity-facing AVA voice coordinator.
 *
 * A user starts a visible listening session explicitly. AVA then accepts
 * multiple one-utterance commands until Stop is pressed or the Activity leaves
 * the foreground. No background microphone or cloud recognition is used.
 */
class AvaVoiceController(
    context: Context,
    private val onListeningChanged: (Boolean) -> Unit = {},
    private val onError: (Throwable) -> Unit = {},
    private val onTranscript: (String) -> Unit = {}
) {
    private val appContext = context.applicationContext
    private val settingsStore = AvaSettingsStore(appContext)
    private val mainHandler = Handler(Looper.getMainLooper())
    private var speechInput: AvaSpeechInput? = null
    @Volatile
    private var continuousListening = false
    private var restartTask: Runnable? = null

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
        startRecognitionSession()
    }

    @MainThread
    private fun startRecognitionSession() {
        if (!continuousListening) return
        val settings = settingsStore.load()
        if (!settings.enabled) {
            stopListening()
            onError(IllegalStateException("AVA is disabled in settings"))
            return
        }

        val input = AvaSpeechInput(
            context = appContext,
            onTranscript = { transcript ->
                if (continuousListening) {
                    try {
                        onTranscript(transcript)
                        AvaCommandClient.submitTranscript(appContext, transcript)
                    } catch (error: Exception) {
                        onError(error)
                    }
                    scheduleNextUtterance(NEXT_UTTERANCE_DELAY_MS)
                }
            },
            onError = { code ->
                if (continuousListening && code == AvaSpeechInput.ERROR_NO_SPEECH) {
                    scheduleNextUtterance(NO_SPEECH_RETRY_DELAY_MS)
                } else if (continuousListening) {
                    continuousListening = false
                    onError(IllegalStateException("Speech recognition failed (code $code)"))
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
    private fun scheduleNextUtterance(delayMs: Long) {
        restartTask?.let(mainHandler::removeCallbacks)
        val task = Runnable {
            if (continuousListening) {
                speechInput?.release()
                speechInput = null
                startRecognitionSession()
            }
        }
        restartTask = task
        mainHandler.postDelayed(task, delayMs)
    }

    @MainThread
    fun stopListening() {
        continuousListening = false
        restartTask?.let(mainHandler::removeCallbacks)
        restartTask = null
        speechInput?.release()
        speechInput = null
    }

    @MainThread
    fun release() = stopListening()

    companion object {
        private const val NEXT_UTTERANCE_DELAY_MS = 700L
        private const val NO_SPEECH_RETRY_DELAY_MS = 250L
    }
}
