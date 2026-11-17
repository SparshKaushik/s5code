package club.touchtech.s5code.kotlin.platform.notifications

import android.content.Context
import club.touchtech.s5code.kotlin.BuildConfig
import com.google.firebase.FirebaseApp
import com.google.firebase.FirebaseOptions
import com.google.firebase.messaging.FirebaseMessaging

/** Public Firebase Android-client configuration for this exact application id. */
data class FirebaseClientConfig(
    val applicationId: String,
    val apiKey: String,
    val projectId: String,
    val senderId: String,
) {
    val configured: Boolean
        get() =
            applicationId.isNotBlank() &&
                apiKey.isNotBlank() &&
                projectId.isNotBlank() &&
                senderId.isNotBlank()

    companion object {
        fun fromBuildConfig() =
            FirebaseClientConfig(
                applicationId = BuildConfig.FIREBASE_APPLICATION_ID.trim(),
                apiKey = BuildConfig.FIREBASE_API_KEY.trim(),
                projectId = BuildConfig.FIREBASE_PROJECT_ID.trim(),
                senderId = BuildConfig.FIREBASE_GCM_SENDER_ID.trim(),
            )
    }
}

/**
 * Initializes Firebase without the Google Services Gradle plugin.
 *
 * The Kotlin package has no checked-in Firebase client entry yet. Explicit
 * configuration lets a maintainer inject the public client values in CI while an
 * ordinary source build remains valid and reports push as unavailable. Borrowing
 * the React Native package's app id would make installation succeed but FIS token
 * creation fail later under the wrong identity.
 */
object S5Firebase {
    fun initialize(context: Context, config: FirebaseClientConfig = FirebaseClientConfig.fromBuildConfig()): Boolean {
        if (!config.configured) return false
        val existing = runCatching { FirebaseApp.getInstance() }.getOrNull()
        if (existing != null) return true
        val options =
            FirebaseOptions.Builder()
                .setApplicationId(config.applicationId)
                .setApiKey(config.apiKey)
                .setProjectId(config.projectId)
                .setGcmSenderId(config.senderId)
                .build()
        FirebaseApp.initializeApp(context, options)
        return true
    }

    @Suppress("DEPRECATION")
    fun requestToken(onResult: (Result<String>) -> Unit) {
        runCatching { FirebaseMessaging.getInstance() }
            .onFailure { onResult(Result.failure(it)) }
            .onSuccess { messaging ->
                messaging.token.addOnCompleteListener { task ->
                    val token = task.result?.trim().orEmpty()
                    when {
                        !task.isSuccessful ->
                            onResult(
                                Result.failure(
                                    task.exception ?: IllegalStateException("FCM token lookup failed.")
                                )
                            )
                        token.isEmpty() ->
                            onResult(Result.failure(IllegalStateException("FCM returned an empty token.")))
                        else -> onResult(Result.success(token))
                    }
                }
            }
    }
}
