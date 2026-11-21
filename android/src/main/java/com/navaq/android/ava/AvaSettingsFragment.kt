package com.navaq.android.ava

import android.os.Bundle
import android.widget.Toast
import androidx.preference.Preference
import androidx.preference.PreferenceCategory
import androidx.preference.PreferenceFragmentCompat
import com.navaq.android.R

/**
 * Embeddable AVA settings page for the host app's existing settings navigation.
 * Uses the same private preference file and keys as AvaSettingsStore.
 */
class AvaSettingsFragment : PreferenceFragmentCompat() {
    override fun onCreatePreferences(savedInstanceState: Bundle?, rootKey: String?) {
        preferenceManager.sharedPreferencesName = "navaq_ava_preferences"
        preferenceManager.sharedPreferencesMode = android.content.Context.MODE_PRIVATE
        setPreferencesFromResource(R.xml.ava_preferences, rootKey)
        addOfflineSpeechModelPreference()
    }

    private fun addOfflineSpeechModelPreference() {
        val manager = AvaSpeechModelManager(requireContext())
        val category = PreferenceCategory(requireContext()).apply {
            title = "Offline voice recognition"
            summary = "Persian and English speech recognition runs on this device"
        }
        preferenceScreen.addPreference(category)

        val modelPreference = Preference(requireContext()).apply {
            key = "ava_install_offline_speech_model"
            title = "Persian + English speech model"
            summary = if (manager.isInstalled()) {
                "Installed · ready to recognize speech without internet"
            } else {
                "Not installed · one-time download (~116 MB), then works offline"
            }
        }
        category.addPreference(modelPreference)

        modelPreference.setOnPreferenceClickListener {
            if (manager.isInstalled()) {
                Toast.makeText(requireContext(), "AVA offline speech model is ready", Toast.LENGTH_SHORT).show()
            } else {
                modelPreference.isEnabled = false
                modelPreference.summary = "Downloading and preparing offline model… 0%"
                manager.install(
                    onProgress = { progress ->
                        activity?.runOnUiThread {
                            modelPreference.summary = "Downloading and preparing offline model… $progress%"
                        }
                    },
                    onComplete = { result ->
                        activity?.runOnUiThread {
                            modelPreference.isEnabled = true
                            if (result.isSuccess) {
                                modelPreference.summary = "Installed · Persian and English recognition is ready offline"
                                Toast.makeText(requireContext(), "AVA offline speech model installed", Toast.LENGTH_LONG).show()
                            } else {
                                val reason = result.exceptionOrNull()?.message ?: "Unknown error"
                                modelPreference.summary = "Installation failed · tap to retry"
                                Toast.makeText(requireContext(), "AVA model download failed: $reason", Toast.LENGTH_LONG).show()
                            }
                        }
                    }
                )
            }
            true
        }
    }
}
