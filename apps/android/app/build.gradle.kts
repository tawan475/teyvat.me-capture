// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

val ndkVersionName = "30.0.16248370"

// One version for the whole release: apps/desktop/Cargo.toml's
// `package.version` (the release workflow tags it). The version code counts
// up with it (1.2.3 -> 10203), as Google Play needs.
val releaseVersion: String = rootProject.file("../desktop/Cargo.toml").readLines()
    .first { it.startsWith("version = ") }
    .substringAfter('"').substringBefore('"')
val releaseVersionCode: Int = releaseVersion.substringBefore('-').split('.')
    .map { it.toInt() }
    .let { (major, minor, patch) -> major * 10_000 + minor * 100 + patch }

android {
    namespace = "me.teyvat.app"
    compileSdk = 36
    ndkVersion = ndkVersionName

    // The app's languages (every one the game ships) in Android 13+'s
    // per-app language setting; res/resources.properties names the default.
    androidResources {
        generateLocaleConfig = true
    }

    defaultConfig {
        applicationId = "me.teyvat.app"
        minSdk = 29
        targetSdk = 36
        versionCode = releaseVersionCode
        versionName = releaseVersion
        ndk { abiFilters += listOf("arm64-v8a") }
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    // Release signing with the upload key (Google Play's "upload key"), from
    // the environment: CI writes the keystore from its secrets
    // (.github/workflows/android.yml). Without it a release build is left
    // unsigned; nothing secret is ever in the repo.
    val keystore = System.getenv("ANDROID_KEYSTORE_PATH")?.takeIf { it.isNotBlank() }
    signingConfigs {
        if (keystore != null) {
            create("upload") {
                storeFile = file(keystore)
                storePassword = System.getenv("ANDROID_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("ANDROID_KEY_ALIAS")
                keyPassword = System.getenv("ANDROID_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        getByName("release") {
            // Not minified: R8 is untested with the JNI bridge.
            isMinifyEnabled = false
            signingConfig = signingConfigs.findByName("upload")
        }
    }

    sourceSets {
        getByName("main") {
            jniLibs.directories.add(layout.buildDirectory.dir("rustJniLibs").get().asFile.path)
            assets.directories.add(layout.buildDirectory.dir("generated/notices").get().asFile.path)
        }
    }
}

// THIRD_PARTY_NOTICES.md from the repo root, shown in Licences.
val notices = tasks.register<Copy>("copyNotices") {
    from(rootProject.file("../../THIRD_PARTY_NOTICES.md"), rootProject.file("../../LICENSE"))
    into(layout.buildDirectory.dir("generated/notices"))
}

// The native forwarder (android/native) built by cargo-ndk into the jniLibs
// folder above. The repo's rust-toolchain.toml picks the toolchain.
val sdkDir = androidComponents.sdkComponents.sdkDirectory
val cargoNdk = tasks.register<Exec>("cargoNdk") {
    val out = layout.buildDirectory.dir("rustJniLibs").get().asFile
    workingDir = rootProject.file("native")
    doFirst { environment("ANDROID_NDK_HOME", sdkDir.get().asFile.resolve("ndk/$ndkVersionName").path) }
    commandLine("cargo", "ndk", "-t", "arm64-v8a", "-P", "29", "-o", out.path, "build", "--release")
}

tasks.named("preBuild") { dependsOn(cargoNdk, notices) }

dependencies {
    val composeBom = platform("androidx.compose:compose-bom:2025.06.01")
    implementation(composeBom)
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    debugImplementation("androidx.compose.ui:ui-tooling")
    implementation("androidx.activity:activity-compose:1.10.1")
    // Custom Tabs for the links (paimon.moe, StarDB, teyvat.me).
    implementation("androidx.browser:browser:1.8.0")
    // FileProvider for Share and Share log.
    implementation("androidx.core:core-ktx:1.16.0")
}
