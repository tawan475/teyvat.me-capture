// SPDX-License-Identifier: MIT
// Copyright (c) 2026 tawan475

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

val ndkVersionName = "30.0.16248370"

android {
    namespace = "me.teyvat.app"
    compileSdk = 36
    ndkVersion = ndkVersionName

    defaultConfig {
        applicationId = "me.teyvat.app"
        minSdk = 29
        targetSdk = 36
        versionCode = 1
        versionName = "1.0.0"
        ndk { abiFilters += listOf("arm64-v8a") }
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    sourceSets {
        getByName("main") {
            jniLibs.directories.add(layout.buildDirectory.dir("rustJniLibs").get().asFile.path)
        }
    }
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

tasks.named("preBuild") { dependsOn(cargoNdk) }

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
