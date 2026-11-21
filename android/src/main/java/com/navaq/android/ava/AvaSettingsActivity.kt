package com.navaq.android.ava

import android.os.Bundle
import androidx.fragment.app.FragmentActivity

/** Hosts the embeddable AVA preferences screen. */
class AvaSettingsActivity : FragmentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.statusBarColor = android.graphics.Color.rgb(10, 15, 29)
        window.navigationBarColor = android.graphics.Color.rgb(10, 15, 29)
        if (savedInstanceState == null) {
            supportFragmentManager.beginTransaction()
                .replace(android.R.id.content, AvaSettingsFragment())
                .commit()
        }
    }
}
