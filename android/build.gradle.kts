plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.navaq.android"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.navaq.android"
        minSdk = 23
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    buildFeatures {
        buildConfig = true
    }
}

dependencies {
    val media3Version = "1.11.1"
    implementation("androidx.core:core-ktx:1.17.0")
    implementation("androidx.media3:media3-exoplayer:$media3Version")
    implementation("androidx.media3:media3-session:$media3Version")
    implementation("androidx.preference:preference-ktx:1.2.1")
    implementation("org.apache.commons:commons-compress:1.27.1")
    implementation(files("libs/sherpa-onnx-release.aar"))
}
