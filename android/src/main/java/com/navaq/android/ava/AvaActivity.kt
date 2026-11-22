package com.navaq.android.ava

import android.Manifest
import android.app.Activity
import android.app.AlertDialog
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.Uri
import android.os.Build
import android.provider.Settings
import android.provider.AlarmClock
import android.provider.MediaStore
import android.content.pm.PackageManager
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.os.Bundle
import android.view.Gravity
import android.view.View
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.ScrollView
import android.widget.TextView
import androidx.core.content.ContextCompat
import com.navaq.android.playback.NavaqPlaybackService
import com.navaq.android.discover.DiscoverActivity
import com.navaq.android.notifications.NavaqNotificationCenter

/**
 * Small, native AVA host screen. It gives the Android application a real,
 * visible entry point for installing the offline model and issuing voice
 * commands without replacing NAVAQ's playback service.
 */
class AvaActivity : Activity() {
    private lateinit var statusText: TextView
    private lateinit var transcriptText: TextView
    private lateinit var modelText: TextView
    private lateinit var modelProgress: ProgressBar
    private lateinit var installButton: Button
    private lateinit var listenButton: Button
    private lateinit var stopButton: Button
    private lateinit var notificationButton: Button
    private lateinit var controller: AvaVoiceController
    private var receiverRegistered = false
    private var userStopRequested = false

    private val resultReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            when (intent?.action) {
                NavaqPlaybackService.ACTION_AVA_RESULT -> {
                    val message = intent.getStringExtra(NavaqPlaybackService.EXTRA_AVA_RESULT)
                    if (!message.isNullOrBlank()) statusText.text = message
                }
                NavaqPlaybackService.ACTION_AVA_SYSTEM_ACTION -> {
                    handleAndroidSystemAction(
                        intent.getStringExtra(NavaqPlaybackService.EXTRA_AVA_SYSTEM_ACTION).orEmpty(),
                        intent.getStringExtra(NavaqPlaybackService.EXTRA_AVA_PHONE_NUMBER).orEmpty()
                    )
                }
                NavaqPlaybackService.ACTION_AVA_CONFIRMATION_REQUIRED -> {
                    val message = intent.getStringExtra(
                        NavaqPlaybackService.EXTRA_AVA_CONFIRMATION_TEXT
                    ) ?: "Confirm the music change?"
                    AlertDialog.Builder(this@AvaActivity)
                        .setTitle("AVA confirmation")
                        .setMessage(message)
                        .setPositiveButton("Confirm") { _, _ ->
                            AvaCommandClient.confirmMusicChange(this@AvaActivity, true)
                        }
                        .setNegativeButton("Cancel") { _, _ ->
                            AvaCommandClient.confirmMusicChange(this@AvaActivity, false)
                        }
                        .show()
                }
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.statusBarColor = Color.rgb(7, 8, 18)
        window.navigationBarColor = Color.rgb(7, 8, 18)
        window.decorView.systemUiVisibility = 0

        NavaqNotificationCenter.createChannels(this)
        controller = AvaVoiceController(
            context = this,
            onListeningChanged = { listening ->
                runOnUiThread {
                    if (listening) {
                        statusText.text = "Listening… say a command, then continue naturally"
                        stopButton.isEnabled = true
                        listenButton.isEnabled = false
                    } else if (controller.isContinuousListening) {
                        statusText.text = "Command captured • AVA is ready for the next one"
                        stopButton.isEnabled = true
                        listenButton.isEnabled = false
                    } else {
                        stopButton.isEnabled = false
                        if (userStopRequested) {
                            userStopRequested = false
                            statusText.text = "Listening stopped."
                        }
                        listenButton.isEnabled = AvaSpeechModelManager(this).isInstalled()
                    }
                }
            },
            onError = { error ->
                runOnUiThread {
                    if (controller.isContinuousListening) {
                        statusText.text = "Couldn't catch that. AVA is listening again."
                        listenButton.isEnabled = false
                        stopButton.isEnabled = true
                    } else {
                        statusText.text = error.message ?: "AVA could not start speech recognition"
                        listenButton.isEnabled = AvaSpeechModelManager(this).isInstalled()
                        stopButton.isEnabled = false
                    }
                }
            },
            onTranscript = { transcript ->
                runOnUiThread {
                    transcriptText.text = "Heard: $transcript"
                    statusText.text = "Speech recognized locally."
                }
            },
            onWakePhraseDetected = {
                runOnUiThread {
                    statusText.text = "AVA activated. Say your command…"
                }
            }
        )

        setContentView(buildContent())
        refreshModelState()
    }

    override fun onStart() {
        super.onStart()
        val filter = IntentFilter().apply {
            addAction(NavaqPlaybackService.ACTION_AVA_RESULT)
            addAction(NavaqPlaybackService.ACTION_AVA_CONFIRMATION_REQUIRED)
            addAction(NavaqPlaybackService.ACTION_AVA_SYSTEM_ACTION)
        }
        ContextCompat.registerReceiver(
            this,
            resultReceiver,
            filter,
            ContextCompat.RECEIVER_NOT_EXPORTED
        )
        receiverRegistered = true
        refreshModelState()
    }

    override fun onStop() {
        controller.stopListening()
        if (receiverRegistered) {
            unregisterReceiver(resultReceiver)
            receiverRegistered = false
        }
        super.onStop()
    }

    override fun onDestroy() {
        controller.release()
        super.onDestroy()
    }

    private fun buildContent(): View {
        val scroll = ScrollView(this).apply {
            setBackgroundColor(Color.rgb(7, 8, 18))
            isFillViewport = true
        }
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(24), dp(32), dp(24), dp(28))
        }
        scroll.addView(root)

        val eyebrow = text("NAVAQ • ON-DEVICE VOICE", 12, 0xFF8FA4C7.toInt(), true)
        root.addView(eyebrow)

        val title = text("AVA", 42, Color.WHITE, true).apply {
            letterSpacing = 0.08f
            setPadding(0, dp(6), 0, 0)
        }
        root.addView(title)

        root.addView(
            text(
                "دستیار صوتی آفلاین  •  Persian + English",
                16,
                0xFFB9C7DF.toInt(),
                false
            ).apply { setPadding(0, dp(4), 0, dp(24)) }
        )

        val statusCard = card()
        statusText = text("Preparing AVA…", 16, Color.WHITE, true)
        transcriptText = text("Your recognized command will appear here. AVA can keep listening for consecutive commands until you press Stop.", 14, 0xFFB9C7DF.toInt(), false)
        statusCard.addView(statusText)
        statusCard.addView(transcriptText.apply { setPadding(0, dp(12), 0, 0) })
        root.addView(statusCard, verticalParams(bottom = 18))

        val modelCard = card()
        modelText = text("Offline speech model status", 15, Color.WHITE, true)
        modelProgress = ProgressBar(this, null, android.R.attr.progressBarStyleHorizontal).apply {
            max = 100
            progress = 0
            visibility = View.GONE
        }
        installButton = button("Install offline speech model", 0xFF253B60.toInt()) {
            installOfflineModel()
        }
        modelCard.addView(modelText)
        modelCard.addView(modelProgress, verticalParams(top = 14, bottom = 12))
        modelCard.addView(installButton)
        root.addView(modelCard, verticalParams(bottom = 18))

        val voiceCard = card()
        voiceCard.addView(text("VOICE COMMAND", 12, 0xFF8FA4C7.toInt(), true))
        listenButton = button("Start listening", 0xFF176B72.toInt()) {
            requestPermissionsAndListen()
        }
        stopButton = button("Stop listening", 0xFF343F56.toInt()) {
            userStopRequested = true
            controller.stopListening()
            statusText.text = "Listening stopped."
            stopButton.isEnabled = false
            listenButton.isEnabled = AvaSpeechModelManager(this).isInstalled()
        }.apply { isEnabled = false }
        voiceCard.addView(listenButton, verticalParams(top = 14, bottom = 8))
        voiceCard.addView(stopButton)
        root.addView(voiceCard, verticalParams(bottom = 18))

        val discoverButton = button("Open NAVAQ Discover", 0xFF176B72.toInt()) {
            startActivity(Intent(this, DiscoverActivity::class.java))
        }
        root.addView(discoverButton, verticalParams(bottom = 10))

        val settingsButton = button("AVA settings", 0xFF253047.toInt()) {
            startActivity(Intent(this, AvaSettingsActivity::class.java))
        }
        root.addView(settingsButton, verticalParams(bottom = 18))

        val notificationCard = card()
        notificationCard.addView(text("SMART NOTIFICATIONS", 12, 0xFF8FA4C7.toInt(), true))
        notificationCard.addView(
            text(
                "Optional alerts for AVA results, playback errors and completed downloads. Android media controls remain managed by the system media session.",
                13,
                0xFFB9C7DF.toInt(),
                false
            ).apply { setPadding(0, dp(8), 0, dp(12)) }
        )
        notificationButton = button(
            if (NavaqNotificationCenter.isEnabled(this)) "Notifications enabled" else "Enable smart notifications",
            0xFF253B60.toInt()
        ) { enableSmartNotifications() }
        notificationCard.addView(notificationButton)
        root.addView(notificationCard, verticalParams(bottom = 12))

        root.addView(
            text(
                "The model downloads once. After installation, speech recognition and command interpretation run locally; no cloud AI is used.",
                12,
                0xFF8292AE.toInt(),
                false
            ).apply { setPadding(0, dp(20), 0, 0) }
        )

        root.alpha = 0f
        root.translationY = dp(12).toFloat()
        root.post {
            root.animate().alpha(1f).translationY(0f).setDuration(260L)
                .setInterpolator(android.view.animation.DecelerateInterpolator()).start()
        }
        return scroll
    }

    private fun installOfflineModel() {
        installButton.isEnabled = false
        listenButton.isEnabled = false
        modelProgress.visibility = View.VISIBLE
        modelProgress.progress = 0
        modelText.text = "Downloading the 116 MB speech model…"
        AvaSpeechModelManager(this).install(
            onProgress = { progress ->
                runOnUiThread {
                    modelProgress.progress = progress
                    modelText.text = "Preparing offline speech model… $progress%"
                }
            },
            onComplete = { result ->
                runOnUiThread {
                    installButton.isEnabled = true
                    if (result.isSuccess) {
                        modelText.text = "Installed • Persian and English are ready offline"
                        installButton.text = "Offline model ready"
                        listenButton.isEnabled = true
                        statusText.text = "AVA is ready. Tap Start listening and speak."
                    } else {
                        val reason = result.exceptionOrNull()?.message ?: "Unknown installation error"
                        modelText.text = "Installation failed: $reason"
                        installButton.text = "Retry model installation"
                        listenButton.isEnabled = false
                    }
                }
            }
        )
    }

    private fun refreshModelState() {
        val installed = AvaSpeechModelManager(this).isInstalled()
        modelText.text = if (installed) {
            "Installed • Persian and English are ready offline"
        } else {
            "Not installed • one-time download (~116 MB)"
        }
        installButton.text = if (installed) "Offline model ready" else "Install offline speech model"
        installButton.isEnabled = !installed
        listenButton.isEnabled = installed
        statusText.text = if (installed) {
            "AVA is ready. Tap Start listening and speak."
        } else {
            "Install the offline speech model to enable voice recognition."
        }
    }


    private fun enableSmartNotifications() {
        if (NavaqNotificationCenter.isEnabled(this)) {
            NavaqNotificationCenter.setEnabled(this, false)
            notificationButton.text = "Enable smart notifications"
            statusText.text = "Smart notifications are disabled."
            return
        }
        if (Build.VERSION.SDK_INT >= 33 &&
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            requestPermissions(
                arrayOf(Manifest.permission.POST_NOTIFICATIONS),
                REQUEST_NOTIFICATION_PERMISSION
            )
            return
        }
        NavaqNotificationCenter.setEnabled(this, true)
        notificationButton.text = "Notifications enabled"
        statusText.text = "Smart notifications are enabled."
    }

    private fun handleAndroidSystemAction(action: String, phoneNumber: String = "") {
        val safeNumber = phoneNumber.takeIf { it.matches(Regex("^\\+?[0-9]{5,18}$")) }.orEmpty()
        val target = when (action) {
            "open_dialer" -> Intent(
                Intent.ACTION_DIAL,
                Uri.parse(if (safeNumber.isBlank()) "tel:" else "tel:$safeNumber")
            )
            "open_wifi_settings" -> Intent(Settings.ACTION_WIFI_SETTINGS)
            "open_bluetooth_settings" -> Intent(Settings.ACTION_BLUETOOTH_SETTINGS)
            "open_battery_settings" -> Intent(Settings.ACTION_BATTERY_SAVER_SETTINGS)
            "open_messages" -> Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_APP_MESSAGING)
            "open_camera" -> Intent(MediaStore.INTENT_ACTION_STILL_IMAGE_CAMERA)
            "open_clock" -> Intent(AlarmClock.ACTION_SHOW_ALARMS)
            "open_notification_settings" -> {
                if (Build.VERSION.SDK_INT >= 26) {
                    Intent(Settings.ACTION_APP_NOTIFICATION_SETTINGS)
                        .putExtra(Settings.EXTRA_APP_PACKAGE, packageName)
                } else {
                    Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS)
                        .setData(Uri.parse("package:$packageName"))
                }
            }
            else -> null
        } ?: return
        try {
            startActivity(target)
        } catch (_: Exception) {
            statusText.text = "This Android screen is not available on this device."
        }
    }

    private fun requestPermissionsAndListen() {
        val settings = AvaSettingsStore(this).load()
        val permissions = mutableListOf(Manifest.permission.RECORD_AUDIO)
        if (settings.musicSources != AvaMusicSources.IN_APP) {
            val audioPermission = AvaPermissions.requiredAudioPermission()
            if (checkSelfPermission(audioPermission) != PackageManager.PERMISSION_GRANTED) {
                permissions += audioPermission
            }
        }
        val missing = permissions.filter {
            checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED
        }
        if (missing.isNotEmpty()) {
            requestPermissions(missing.toTypedArray(), REQUEST_AVA_PERMISSIONS)
        } else {
            beginListening()
        }
    }

    private fun beginListening() {
        try {
            transcriptText.text = "Waiting for your voice…"
            listenButton.isEnabled = false
            stopButton.isEnabled = true
            statusText.text = "Preparing the offline recognizer…"
            controller.startListening()
            stopButton.isEnabled = true
            listenButton.isEnabled = false
        } catch (error: Exception) {
            statusText.text = error.message ?: "AVA could not start listening"
            listenButton.isEnabled = AvaSpeechModelManager(this).isInstalled()
            stopButton.isEnabled = false
        }
    }

    @Deprecated("Use the Activity Result API when the host navigation is modernized")
    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == REQUEST_NOTIFICATION_PERMISSION) {
            val granted = checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) ==
                PackageManager.PERMISSION_GRANTED
            if (granted) {
                NavaqNotificationCenter.setEnabled(this, true)
                notificationButton.text = "Notifications enabled"
                statusText.text = "Smart notifications are enabled."
            } else {
                notificationButton.text = "Enable smart notifications"
                statusText.text = "Notifications were not enabled. You can change this in Android settings."
            }
            return
        }
        if (requestCode == REQUEST_AVA_PERMISSIONS) {
            val microphoneGranted = checkSelfPermission(Manifest.permission.RECORD_AUDIO) ==
                PackageManager.PERMISSION_GRANTED
            if (microphoneGranted) {
                beginListening()
            } else {
                statusText.text = "Microphone permission is required to use AVA."
            }
        }
    }

    private fun card(): LinearLayout = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(dp(18), dp(18), dp(18), dp(18))
        background = GradientDrawable(
            GradientDrawable.Orientation.TL_BR,
            intArrayOf(Color.rgb(23, 34, 59), Color.rgb(12, 18, 34))
        ).apply {
            cornerRadius = dp(22).toFloat()
            setStroke(dp(1), Color.argb(115, 98, 211, 255))
        }
        elevation = dp(2).toFloat()
    }

    private fun text(value: String, size: Int, color: Int, bold: Boolean): TextView =
        TextView(this).apply {
            text = value
            textSize = size.toFloat()
            setTextColor(color)
            typeface = if (bold) Typeface.DEFAULT_BOLD else Typeface.DEFAULT
            gravity = Gravity.START
        }

    private fun button(label: String, color: Int, action: () -> Unit): Button =
        Button(this).apply {
            text = label
            isAllCaps = false
            textSize = 15f
            setTextColor(Color.WHITE)
            background = GradientDrawable(
                GradientDrawable.Orientation.LEFT_RIGHT,
                intArrayOf(color, Color.rgb(78, 62, 145))
            ).apply {
                cornerRadius = dp(15).toFloat()
                setStroke(dp(1), Color.argb(95, 110, 222, 255))
            }
            elevation = dp(2).toFloat()
            setPadding(dp(16), dp(10), dp(16), dp(10))
            isFocusable = true
            setOnTouchListener { view, event ->
                when (event.action) {
                    android.view.MotionEvent.ACTION_DOWN -> {
                        view.animate().cancel()
                        view.animate().scaleX(0.985f).scaleY(0.985f).alpha(0.88f).setDuration(85L).start()
                    }
                    android.view.MotionEvent.ACTION_UP, android.view.MotionEvent.ACTION_CANCEL -> {
                        view.animate().cancel()
                        view.animate().scaleX(1f).scaleY(1f).alpha(1f).setDuration(140L).start()
                    }
                }
                false
            }
            setOnClickListener { action() }
        }

    private fun verticalParams(
        top: Int = 0,
        bottom: Int = 0
    ): LinearLayout.LayoutParams =
        LinearLayout.LayoutParams(
            LinearLayout.LayoutParams.MATCH_PARENT,
            LinearLayout.LayoutParams.WRAP_CONTENT
        ).apply {
            topMargin = dp(top)
            bottomMargin = dp(bottom)
        }

    private fun dp(value: Int): Int = (value * resources.displayMetrics.density).toInt()

    companion object {
        private const val REQUEST_AVA_PERMISSIONS = 7104
        private const val REQUEST_NOTIFICATION_PERMISSION = 7105
    }
}
