package com.navaq.android.ava

import android.content.Context
import android.content.Intent
import com.navaq.android.playback.NavaqPlaybackService

/** Submit speech recognized from a visible, user-initiated app interaction. */
object AvaCommandClient {
    fun submitTranscript(context: Context, transcript: String) {
        require(transcript.isNotBlank()) { "AVA transcript must not be blank" }
        val intent = Intent(context, NavaqPlaybackService::class.java)
            .setAction(NavaqPlaybackService.ACTION_EXECUTE_AVA)
            .putExtra(NavaqPlaybackService.EXTRA_AVA_TRANSCRIPT, transcript)
        context.startService(intent)
    }
}
