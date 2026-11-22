package com.navaq.android.ava

import android.app.AlertDialog
import android.os.Bundle
import android.view.View
import android.widget.Toast
import androidx.preference.Preference
import androidx.preference.PreferenceCategory
import androidx.preference.PreferenceFragmentCompat
import com.navaq.android.R
import java.util.Locale

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

    override fun onViewCreated(view: View, savedInstanceState: Bundle?) {
        super.onViewCreated(view, savedInstanceState)
        view.setBackgroundColor(android.graphics.Color.rgb(7, 8, 18))
        view.importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_YES
        view.alpha = 0f
        view.translationY = resources.displayMetrics.density * 8f
        view.post {
            view.animate().alpha(1f).translationY(0f).setDuration(220L)
                .setInterpolator(android.view.animation.DecelerateInterpolator()).start()
        }
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
            modelPreference.summary = when {
                manager.isInstalled() -> {
                    val sizeMb = manager.installedSizeBytes() / (1024.0 * 1024.0)
                    "Downloaded · %.1f MB · works offline · tap to manage".format(Locale.US, sizeMb)
                }
                manager.hasPendingDownload() -> {
                    val sizeMb = manager.pendingDownloadSizeBytes() / (1024.0 * 1024.0)
                    "Interrupted download · %.1f MB saved · tap to resume or remove".format(Locale.US, sizeMb)
                }
                else -> "Not downloaded · about 116 MB · tap to download"
            }
        }
        refreshSummary()

        modelPreference.setOnPreferenceClickListener {
            when {
                manager.isInstalled() -> {
                    AlertDialog.Builder(requireContext())
                        .setTitle("Manage AVA speech model")
                        .setMessage(
                            "The offline Persian and English speech model uses " +
                                "%.1f MB of storage. Removing it frees this space. " +
                                "AVA's other features remain available, but offline speech recognition " +
                                "will require downloading the model again.".format(
                                    Locale.US,
                                    manager.installedSizeBytes() / (1024.0 * 1024.0)
                                )
                        )
                        .setNegativeButton("Keep model", null)
                        .setPositiveButton("Remove download") { _, _ ->
                            val removed = manager.deleteModel()
                            refreshSummary()
                            val message = if (removed) {
                                "AVA speech model and saved download data removed"
                            } else {
                                "The model could not be removed while an installation is running"
                            }
                            Toast.makeText(requireContext(), message, Toast.LENGTH_LONG).show()
                        }
                        .show()
                }
                manager.hasPendingDownload() -> {
                    val sizeMb = manager.pendingDownloadSizeBytes() / (1024.0 * 1024.0)
                    AlertDialog.Builder(requireContext())
                        .setTitle("Interrupted AVA download")
                        .setMessage(
                            "%.1f MB of the optional speech model is saved. You can safely resume " +
                                "the download or remove the saved data.".format(Locale.US, sizeMb)
                        )
                        .setNegativeButton("Remove saved data") { _, _ ->
                            val removed = manager.deletePendingDownload()
                            refreshSummary()
                            Toast.makeText(
                                requireContext(),
                                if (removed) "Saved AVA download data removed" else "Could not remove saved download data",
                                Toast.LENGTH_LONG
                            ).show()
                        }
                        .setPositiveButton("Resume download") { _, _ ->
                            startModelInstall(manager, modelPreference, ::refreshSummary)
                        }
                        .show()
                }
                else -> startModelInstall(manager, modelPreference, ::refreshSummary)
            }
            true
        }
    }

    private fun startModelInstall(
        manager: AvaSpeechModelManager,
        modelPreference: Preference,
        refreshSummary: () -> Unit
    ) {
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
                            refreshSummary()
                            val reason = result.exceptionOrNull()?.message ?: "Unknown error"
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
}
