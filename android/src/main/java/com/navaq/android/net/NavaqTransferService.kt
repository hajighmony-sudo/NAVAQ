package com.navaq.android.net

import android.app.Notification
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import androidx.core.app.NotificationCompat
import com.navaq.android.notifications.NavaqNotificationCenter
import com.navaq.android.ui.NavaqHomeActivity

/**
 * Keeps downloads and shared listening rooms alive while the app is in the background.
 * It stops itself as soon as there is nothing left to do.
 */
class NavaqTransferService : Service() {
    private val handler = Handler(Looper.getMainLooper())
    private val check = object : Runnable {
        override fun run() {
            val text = status()
            if (text == null) {
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
                return
            }
            getSystemService(NOTIFICATION_SERVICE).let { (it as android.app.NotificationManager).notify(ID, build(text)) }
            handler.postDelayed(this, 2000L)
        }
    }

    private fun status(): String? {
        val active = LinkDownloader.list(this).count { it.state == LinkDownloader.RUNNING || it.state == LinkDownloader.QUEUED }
        val parts = mutableListOf<String>()
        if (active > 0) parts += "$active دانلود فعال"
        LanRoom.host?.let { parts += "اتاق مشترک: ${it.guestCount} مهمان" }
        if (LanRoom.guest != null) parts += "متصل به اتاق مشترک"
        return if (parts.isEmpty()) null else parts.joinToString(" • ")
    }

    private fun build(text: String): Notification {
        val open = PendingIntent.getActivity(
            this, 0, Intent(this, NavaqHomeActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )
        return NotificationCompat.Builder(this, NavaqNotificationCenter.CHANNEL_DOWNLOADS)
            .setSmallIcon(android.R.drawable.stat_sys_download)
            .setContentTitle("NAVAQ")
            .setContentText(text)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setContentIntent(open)
            .build()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        NavaqNotificationCenter.createChannels(this)
        val notification = build(status() ?: "در حال آماده‌سازی…")
        try {
            if (Build.VERSION.SDK_INT >= 29) {
                startForeground(ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC)
            } else {
                startForeground(ID, notification)
            }
        } catch (_: Exception) {
            stopSelf()
            return START_NOT_STICKY
        }
        handler.removeCallbacks(check)
        handler.postDelayed(check, 1500L)
        return START_NOT_STICKY
    }

    override fun onDestroy() {
        handler.removeCallbacksAndMessages(null)
        super.onDestroy()
    }

    override fun onBind(intent: Intent?): IBinder? = null

    companion object {
        private const val ID = 4207

        fun ensure(context: Context) {
            try {
                val intent = Intent(context.applicationContext, NavaqTransferService::class.java)
                androidx.core.content.ContextCompat.startForegroundService(context.applicationContext, intent)
            } catch (_: Exception) {
                // Background-start restrictions: work continues while the app is open.
            }
        }
    }
}
