package com.navaq.android.ava

import android.app.AlertDialog
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
            summary = "Download the speech model only if you want fully offline Persian and English recognition"
        }
        preferenceScreen.addPreference(category)

        val infoPreference = Preference(requireContext()).apply {
            title = "Optional download"
            summary = "The speech model is kept outside the APK. It is downloaded on demand and can be removed whenever you need the storage."
            isSelectable = false
        }
        category.addPreference(infoPreference)

        val modelPreference = Preference(requireContext()).apply {
            key = "ava_install_offline_speech_model"
            title = "Persian + English speech model"
        }
        category.addPreference(modelPreference)

        fun refreshSummary() {
            modelPreference.summary = if (manager.isInstalled()) {
                val sizeMb = manager.installedSizeBytes() / (1024.0 * 1024.0)
                "Downloaded · %.1f MB · works offline · tap to manage".format(java.util.Locale.US, sizeMb)
            } else {
                "Not downloaded · about 116 MB · tap to download"
            }
        }
        refreshSummary()

        modelPreference.setOnPreferenceClickListener {
            if (manager.isInstalled()) {
                AlertDialog.Builder(requireContext())
                    .setTitle("Manage AVA speech model")
                    .setMessage(
                        "The offline Persian and English speech model uses " +
                            "%.1f MB of storage. Removing it frees this space. " +
                            "AVA's other features remain available, but offline speech recognition " +
                            "will require downloading the model again.".format(
                                java.util.Locale.US,
                                manager.installedSizeBytes() / (1024.0 * 1024.0)
                            )
                    )
                    .setNegativeButton("Keep model", null)
                    .setPositiveButton("Remove download") { _, _ ->
                        val removed = manager.deleteModel()
                        refreshSummary()
                        val message = if (removed) {
                            "AVA speech model removed"
                        } else {
                            "The model could not be removed while an installation is running"
                        }
                        Toast.makeText(requireContext(), message, Toast.LENGTH_LONG).show()
                    }
                    .show()
            } else {
                modelPreference.isEnabled = false
                modelPreference.summary = "Downloading and preparing offline model… 0%"
                manager.install(
                    onProgress = { progress ->
                        activity?.runOnUiThread {
                            if (isAdded) {
                                modelPreference.summary = "Downloading and preparing offline model… $progress%"
                            }
                        }
                    },
                    onComplete = { result ->
                        activity?.runOnUiThread {
                            if (isAdded) {
                                modelPreference.isEnabled = true
                                if (result.isSuccess) {
                                    refreshSummary()
                                    Toast.makeText(
                                        requireContext(),
                                        "AVA offline speech recognition is ready",
                                        Toast.LENGTH_LONG
                                    ).show()
                                } else {
                                    val reason = result.exceptionOrNull()?.message ?: "Unknown error"
                                    modelPreference.summary = "Download failed · tap to retry"
                                    Toast.makeText(
                                        requireContext(),
                                        "AVA model download failed: $reason",
                                        Toast.LENGTH_LONG
                                    ).show()
                                }
                            }
                        }
                    }
                )
            }
            true
        }
    }
}}
