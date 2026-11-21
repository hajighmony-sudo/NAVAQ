package com.navaq.android.ava

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.speech.RecognitionListener
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer
import androidx.annotation.MainThread
import androidx.core.content.ContextCompat
import java.util.Locale

/**
 * Explicit, user-started speech capture adapter.
 *
 * Call startListening only from a visible, user-initiated UI flow after the
 * RECORD_AUDIO permission has been granted. Android restricts background
 * microphone service startup; this class intentionally does not implement
 * hidden or permanent microphone capture.
 */
class AvaSpeechInput(
    context: Context,
    private val onTranscript: (String) -> Unit,
    private val onError: (Int) -> Unit,
    private val onListeningChanged: (Boolean) -> Unit = {}
) {
    private val appContext = context.applicationContext
    private var recognizer: SpeechRecognizer? = null

    @MainThread
    fun startListening(languageTag: String = Locale.getDefault().toLanguageTag(), preferOffline: Boolean) {
        check(ContextCompat.checkSelfPermission(appContext, Manifest.permission.RECORD_AUDIO) ==
            PackageManager.PERMISSION_GRANTED) {
            "AVA requires microphone permission before listening"
        }
        check(SpeechRecognizer.isRecognitionAvailable(appContext)) {
            "No speech recognition service is available on this device"
        }

        stopListening()

        recognizer = if (
            preferOffline &&
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.S &&
            SpeechRecognizer.isOnDeviceRecognitionAvailable(appContext)
        ) {
            SpeechRecognizer.createOnDeviceSpeechRecognizer(appContext)
        } else {
            SpeechRecognizer.createSpeechRecognizer(appContext)
        }

        recognizer?.setRecognitionListener(object : RecognitionListener {
            override fun onReadyForSpeech(params: Bundle?) = onListeningChanged(true)
            override fun onBeginningOfSpeech() = Unit
            override fun onRmsChanged(rmsdB: Float) = Unit
            override fun onBufferReceived(buffer: ByteArray?) = Unit
            override fun onEndOfSpeech() = onListeningChanged(false)
            override fun onError(error: Int) {
                onListeningChanged(false)
                onError(error)
            }
            override fun onResults(results: Bundle?) {
                onListeningChanged(false)
                val transcript = results
                    ?.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)
                    ?.firstOrNull()
                    ?.trim()
                    .orEmpty()
                if (transcript.isNotEmpty()) onTranscript(transcript)
            }
            override fun onPartialResults(partialResults: Bundle?) = Unit
            override fun onEvent(eventType: Int, params: Bundle?) = Unit
        })

        val request = Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).apply {
            putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
            putExtra(RecognizerIntent.EXTRA_LANGUAGE, languageTag)
            putExtra(RecognizerIntent.EXTRA_PARTIAL_RESULTS, false)
            putExtra(RecognizerIntent.EXTRA_PREFER_OFFLINE, preferOffline)
            putExtra(RecognizerIntent.EXTRA_MAX_RESULTS, 3)
        }
        recognizer?.startListening(request)
    }

    @MainThread
    fun stopListening() {
        recognizer?.cancel()
        recognizer?.destroy()
        recognizer = null
        onListeningChanged(false)
    }

    @MainThread
    fun release() {
        stopListening()
    }
}
