import java.util.Properties

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.kotlin.serialization)
}

// Release signing uses the EAS-managed keystore for club.touchtech.s5code.kotlin.
// `Scripts/fetch-eas-keystore.sh` materializes it into .credentials/ (gitignored);
// EAS Build injects the same keystore through these env vars in CI.
val credentialsFile = rootProject.file(".credentials/keystore.properties")
val releaseCredentials =
    Properties().apply {
        if (credentialsFile.exists()) {
            credentialsFile.inputStream().use(::load)
        }
    }

fun credential(key: String, env: String): String? =
    (System.getenv(env) ?: releaseCredentials.getProperty(key))?.takeIf(String::isNotBlank)

// Public S5 Connect configuration, read from the same repo-root `.env` contract
// every other client uses (`scripts/lib/public-config.ts`). These are public
// identifiers, not secrets: an unset value builds with cloud features disabled,
// the same way `hasCloudPublicConfig()` gates the React Native client.
val repoEnv =
    Properties().apply {
        listOf(".env", ".env.local").forEach { name ->
            val file = rootProject.file("../../$name")
            if (file.exists()) file.inputStream().use(::load)
        }
    }

fun publicConfig(vararg names: String): String =
    names.firstNotNullOfOrNull { name ->
        (System.getenv(name) ?: repoEnv.getProperty(name))?.trim()?.takeIf(String::isNotEmpty)
    } ?: ""

val releaseStoreFile = credential("storeFile", "S5_KOTLIN_KEYSTORE_PATH")?.let(rootProject::file)
val releaseStorePassword = credential("storePassword", "S5_KOTLIN_KEYSTORE_PASSWORD")
val releaseKeyAlias = credential("keyAlias", "S5_KOTLIN_KEY_ALIAS")
val releaseKeyPassword = credential("keyPassword", "S5_KOTLIN_KEY_PASSWORD")
val hasReleaseSigning =
    releaseStoreFile?.exists() == true &&
        releaseStorePassword != null &&
        releaseKeyAlias != null &&
        releaseKeyPassword != null

android {
    namespace = "club.touchtech.s5code.kotlin"
    compileSdk = 37

    androidResources {
        localeFilters += listOf("en")
    }

    defaultConfig {
        // One production identity for every build type: eas-cli reads the
        // application id from this file and cannot resolve suffixes.
        applicationId = "club.touchtech.s5code.kotlin"
        minSdk = 26
        targetSdk = 37
        versionCode = 1
        versionName = "0.1.0-alpha.1"

        // Same three values the RN client puts in `extra`: publishable key, the
        // JWT template the relay accepts, and the relay origin. Empty means
        // "S5 Connect not configured", which the UI reports rather than failing.
        buildConfigField(
            "String",
            "CLERK_PUBLISHABLE_KEY",
            "\"${publicConfig("T3CODE_CLERK_PUBLISHABLE_KEY", "EXPO_PUBLIC_CLERK_PUBLISHABLE_KEY")}\"",
        )
        buildConfigField(
            "String",
            "CLERK_JWT_TEMPLATE",
            "\"${publicConfig("T3CODE_CLERK_JWT_TEMPLATE", "EXPO_PUBLIC_CLERK_JWT_TEMPLATE")}\"",
        )
        buildConfigField(
            "String",
            "RELAY_URL",
            "\"${publicConfig("T3CODE_RELAY_URL", "VITE_T3CODE_RELAY_URL")}\"",
        )
        // Public Firebase Android client values. A maintainer can supply these
        // through CI or `.env.local` after registering this exact package in the
        // S5 Code Firebase project. Empty values keep FCM disabled honestly.
        buildConfigField(
            "String",
            "FIREBASE_APPLICATION_ID",
            "\"${publicConfig("T3CODE_FIREBASE_ANDROID_APP_ID", "S5_KOTLIN_FIREBASE_APP_ID")}\"",
        )
        buildConfigField(
            "String",
            "FIREBASE_API_KEY",
            "\"${publicConfig("T3CODE_FIREBASE_ANDROID_API_KEY", "S5_KOTLIN_FIREBASE_API_KEY")}\"",
        )
        buildConfigField(
            "String",
            "FIREBASE_PROJECT_ID",
            "\"${publicConfig("T3CODE_FIREBASE_PROJECT_ID", "S5_KOTLIN_FIREBASE_PROJECT_ID")}\"",
        )
        buildConfigField(
            "String",
            "FIREBASE_GCM_SENDER_ID",
            "\"${publicConfig("T3CODE_FIREBASE_GCM_SENDER_ID", "S5_KOTLIN_FIREBASE_GCM_SENDER_ID")}\"",
        )
    }

    signingConfigs {
        if (hasReleaseSigning) {
            create("release") {
                storeFile = releaseStoreFile
                storePassword = releaseStorePassword
                keyAlias = releaseKeyAlias
                keyPassword = releaseKeyPassword
                enableV1Signing = true
                enableV2Signing = true
            }
        }
    }

    buildTypes {
        debug {
            isMinifyEnabled = false
        }
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
            )
            signingConfig = signingConfigs.findByName("release")
        }
    }

    externalNativeBuild {
        cmake {
            path = file("src/main/cpp/CMakeLists.txt")
            version = "3.22.1"
        }
    }

    sourceSets {
        named("main") { java.srcDirs("src/main/kotlin") }
        named("test") {
            java.srcDirs("src/test/kotlin")
            resources.srcDirs("src/main/assets")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    packaging {
        resources.excludes += setOf("/META-INF/{AL2.0,LGPL2.1}")
    }

    lint {
        warningsAsErrors = true
        abortOnError = true
        // Alpha Compose Material 3 is a deliberate, documented pin.
        disable += setOf("GradleDependency", "AndroidGradlePluginVersion", "NewerVersionAvailable")
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
        freeCompilerArgs.addAll("-Xjvm-default=all")
    }
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.clerk.android.ui)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.navigation.compose)
    implementation(libs.androidx.graphics.shapes)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.kotlinx.serialization.json)
    implementation(libs.okhttp)
    implementation(libs.androidx.camera.core)
    implementation(libs.androidx.camera.camera2)
    implementation(libs.androidx.camera.lifecycle)
    implementation(libs.androidx.camera.view)
    implementation(libs.mlkit.barcode.scanning)
    implementation(libs.coil.compose)
    implementation(libs.coil.network.okhttp)
    implementation(libs.firebase.messaging)
    implementation(libs.kotlin.textmate.core)

    implementation(platform(libs.compose.bom))
    implementation(libs.compose.foundation)
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.graphics)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.compose.material3)
    implementation(libs.compose.material3.windowsizeclass)
    implementation(libs.compose.material.icons.extended)
    debugImplementation(libs.compose.ui.tooling)

    testImplementation(libs.junit)
    testImplementation(libs.kotlinx.coroutines.test)
    testImplementation(libs.okhttp.mockwebserver)
}
