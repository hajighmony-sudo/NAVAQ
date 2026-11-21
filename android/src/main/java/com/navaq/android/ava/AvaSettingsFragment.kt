package com.navaq.android.ava

import android.os.Bundle
import androidx.preference.PreferenceFragmentCompat

/**
 * Embeddable AVA settings page for the host app's existing settings navigation.
 * Uses the same private preference file and keys as AvaSettingsStore.
 */
class AvaSettingsFragment : PreferenceFragmentCompat() {
    override fun onCreatePreferences(savedInstanceState: Bundle?, rootKey: String?) {
        preferenceManager.sharedPreferencesName = "navaq_ava_preferences"
        preferenceManager.sharedPreferencesMode = android.content.Context.MODE_PRIVATE
        setPreferencesFromResource(R.xml.ava_preferences, rootKey)
    }
}
