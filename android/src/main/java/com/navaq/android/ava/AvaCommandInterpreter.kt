package com.navaq.android.ava

import android.content.Context
import com.navaq.android.core.NavaqNativeBridge
import org.json.JSONObject

/**
 * Thin Android adapter around the Rust AVA intent engine.
 * The host app remains responsible for resolving music queries against the
 * selected device/in-app libraries and forwarding transport commands to the
 * existing NAVAQ playback service.
 */
class AvaCommandInterpreter(context: Context) {
    private val appContext = context.applicationContext
    private val settingsStore = AvaSettingsStore(appContext)

    fun interpret(transcript: String): JSONObject {
        val settings = settingsStore.load()
        if (!settings.enabled) {
            return JSONObject().put("error", "ava_disabled")
        }
        val result = NavaqNativeBridge.nativeInterpretAva(
            transcript,
            settings.toWireJson(),
            false
        )
        return JSONObject(result)
    }

    fun currentSettings(): AvaSettings = settingsStore.load()

    fun saveSettings(settings: AvaSettings) = settingsStore.save(settings)

}
