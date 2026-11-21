package com.navaq.android.ava

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.content.ContextCompat

/** Runtime permission helpers for the host Activity's permission launcher. */
object AvaPermissions {
    fun requiredAudioPermission(): String =
        if (Build.VERSION.SDK_INT >= 33) Manifest.permission.READ_MEDIA_AUDIO
        else Manifest.permission.READ_EXTERNAL_STORAGE

    fun hasAudioLibraryAccess(context: Context): Boolean =
        ContextCompat.checkSelfPermission(
            context,
            requiredAudioPermission()
        ) == PackageManager.PERMISSION_GRANTED

    fun microphonePermission(): String = Manifest.permission.RECORD_AUDIO

    fun hasMicrophoneAccess(context: Context): Boolean =
        ContextCompat.checkSelfPermission(
            context,
            microphonePermission()
        ) == PackageManager.PERMISSION_GRANTED
}
