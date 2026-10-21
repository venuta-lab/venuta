plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "io.github.venuta_lab.spike"
    compileSdk = 35
    ndkVersion = "29.0.14206865"

    defaultConfig {
        applicationId = "io.github.venuta_lab.spike"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.0.1-spike"
        ndk { abiFilters += listOf("arm64-v8a") }
    }
    buildTypes {
        release {
            isMinifyEnabled = false
            signingConfig = signingConfigs.getByName("debug")
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    sourceSets["main"].java.srcDir("src/main/java-gen")
}

dependencies {
    implementation("net.java.dev.jna:jna:5.17.0@aar")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")
}
