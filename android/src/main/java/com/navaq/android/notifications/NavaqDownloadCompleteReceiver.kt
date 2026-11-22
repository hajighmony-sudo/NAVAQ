package com.navaq.android.notifications

import android.app.DownloadManager
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/**
 * Converts only NAVAQ-owned DownloadManager jobs into app notifications.
 * The download ID is checked against NAVAQ's private allow-list before querying.
 */
class NavaqDownloadCompleteReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent?) {
        if (intent?.action != DownloadManager.ACTION_DOWNLOAD_COMPLETE) return
        val id = intent.getLongExtra(DownloadManager.EXTRA_DOWNLOAD_ID, -1L)
        if (id <= 0L || !NavaqNotificationCenter.isEnabled(context)) return

        val knownIds = context.getSharedPreferences("navaq_discover", Context.MODE_PRIVATE)
            .getStringSet("download_manager_ids", emptySet()).orEmpty()
        if (id.toString() !in knownIds) return

        val manager = context.getSystemService(Context.DOWNLOAD_SERVICE) as DownloadManager
        val cursor = manager.query(DownloadManager.Query().setFilterById(id)) ?: return
        cursor.use {
            if (!it.moveToFirst()) return
            val status = it.getInt(it.getColumnIndexOrThrow(DownloadManager.COLUMN_STATUS))
            if (status != DownloadManager.STATUS_SUCCESSFUL &&
                status != DownloadManager.STATUS_FAILED
            ) return
            val title = it.getString(it.getColumnIndexOrThrow(DownloadManager.COLUMN_TITLE))
                ?.takeIf(String::isNotBlank) ?: "NAVAQ audio download"
            NavaqNotificationCenter.downloadComplete(
                context,
                title,
                status == DownloadManager.STATUS_SUCCESSFUL
            )
        }
    }
}
