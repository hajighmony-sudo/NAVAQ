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
    }

    // Keep ABI-specific outputs for local/testing workflows, and also build
    // one universal APK containing every supported native ABI.
    // The GitHub download workflow publishes only this universal APK.
    splits {
        abi {
            isEnable = true
            reset()
            include("arm64-v8a", "x86_64")
            isUniversalApk = true
        }
    }

    buildTypes {
        getByName("release") {
            // Strip unused Java/Kotlin code and unused Android resources from
            // production builds. Debug builds remain installable for GitHub sideloading.
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"))
        }
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
    implementation("com.google.guava:guava:33.4.0-android")
    implementation("androidx.preference:preference-ktx:1.2.1")
    implementation("org.apache.commons:commons-compress:1.27.1")
    implementation(files("libs/sherpa-onnx-release.aar"))
}
