package com.navaq.android.ava

import android.content.Context
import android.net.ConnectivityManager
import android.net.NetworkCapabilities
import android.os.Build
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
            isNetworkAvailable()
        )
        return JSONObject(result)
    }

    fun currentSettings(): AvaSettings = settingsStore.load()

    fun saveSettings(settings: AvaSettings) = settingsStore.save(settings)

    private fun isNetworkAvailable(): Boolean {
        val manager = appContext.getSystemService(Context.CONNECTIVITY_SERVICE) as? ConnectivityManager
            ?: return false
        val network = manager.activeNetwork ?: return false
        val capabilities = manager.getNetworkCapabilities(network) ?: return false
        return capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_INTERNET) &&
            capabilities.hasCapability(NetworkCapabilities.NET_CAPABILITY_VALIDATED)
    }
}
