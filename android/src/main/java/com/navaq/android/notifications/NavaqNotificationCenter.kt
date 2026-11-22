package com.navaq.android.notifications

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import com.navaq.android.R
import com.navaq.android.ava.AvaActivity
import com.navaq.android.discover.DiscoverActivity

/**
 * Central, opt-in notification surface. Media playback controls are intentionally
 * left to Media3's MediaSession notification to avoid duplicate player controls.
 */
object NavaqNotificationCenter {
    const val PREFS = "navaq_notifications"
    const val KEY_ENABLED = "notifications_enabled"
    const val CHANNEL_ACTIVITY = "navaq_activity"
    const val CHANNEL_DOWNLOADS = "navaq_downloads"
    const val CHANNEL_ERRORS = "navaq_errors"

    fun createChannels(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        manager.createNotificationChannels(
            listOf(
                NotificationChannel(
                    CHANNEL_ACTIVITY,
                    "AVA and app activity",
                    NotificationManager.IMPORTANCE_DEFAULT
                ).apply { description = "Voice assistant results and useful app updates" },
                NotificationChannel(
                    CHANNEL_DOWNLOADS,
                    "Downloads",
                    NotificationManager.IMPORTANCE_DEFAULT
                ).apply { description = "Completed and failed NAVAQ downloads" },
                NotificationChannel(
                    CHANNEL_ERRORS,
                    "Playback alerts",
                    NotificationManager.IMPORTANCE_HIGH
                ).apply { description = "Playback errors that need attention" }
            )
        )
    }

    fun isEnabled(context: Context): Boolean =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .getBoolean(KEY_ENABLED, false)

    fun setEnabled(context: Context, enabled: Boolean) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit().putBoolean(KEY_ENABLED, enabled).apply()
        createChannels(context)
    }

    fun canPost(context: Context): Boolean {
        if (!isEnabled(context)) return false
        return Build.VERSION.SDK_INT < 33 ||
            ContextCompat.checkSelfPermission(
                context,
                Manifest.permission.POST_NOTIFICATIONS
            ) == PackageManager.PERMISSION_GRANTED
    }

    fun avaResult(context: Context, message: String) {
        post(
            context = context,
            channel = CHANNEL_ACTIVITY,
            id = 4101,
            title = "NAVAQ • AVA",
            message = message,
            target = AvaActivity::class.java,
            icon = android.R.drawable.ic_btn_speak_now
        )
    }

    fun playbackError(context: Context, message: String) {
        post(
            context = context,
            channel = CHANNEL_ERRORS,
            id = 4102,
            title = "Playback needs attention",
            message = message,
            target = AvaActivity::class.java,
            icon = android.R.drawable.stat_notify_error
        )
    }

    fun downloadComplete(context: Context, title: String, successful: Boolean) {
        post(
            context = context,
            channel = CHANNEL_DOWNLOADS,
            id = (title.hashCode() and 0x7fffffff).coerceAtLeast(1),
            title = if (successful) "Download ready" else "Download failed",
            message = title,
            target = DiscoverActivity::class.java,
            icon = if (successful) android.R.drawable.stat_sys_download_done
                else android.R.drawable.stat_notify_error
        )
    }

    private fun post(
        context: Context,
        channel: String,
        id: Int,
        title: String,
        message: String,
        target: Class<*>,
        icon: Int
    ) {
        if (!canPost(context)) return
        createChannels(context)
        val intent = Intent(context, target).addFlags(
            Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP
        )
        val pending = PendingIntent.getActivity(
            context,
            id,
            intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        val notification = NotificationCompat.Builder(context, channel)
            .setSmallIcon(icon)
            .setContentTitle(title)
            .setContentText(message.take(180))
            .setStyle(NotificationCompat.BigTextStyle().bigText(message.take(500)))
            .setContentIntent(pending)
            .setAutoCancel(true)
            .setOnlyAlertOnce(true)
            .setPriority(NotificationCompat.PRIORITY_DEFAULT)
            .build()
        NotificationManagerCompat.from(context).notify(id, notification)
    }
}
