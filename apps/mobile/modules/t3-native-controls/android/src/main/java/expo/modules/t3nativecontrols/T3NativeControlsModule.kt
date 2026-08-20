package expo.modules.t3nativecontrols

import expo.modules.kotlin.modules.Module
import expo.modules.kotlin.modules.ModuleDefinition

class T3NativeControlsModule : Module() {
  override fun definition() = ModuleDefinition {
    Name("T3NativeControls")

    Function("canPostAndroidLiveUpdates") {
      AndroidLiveUpdateNotifications.canPostPromoted(
        requireNotNull(appContext.reactContext).applicationContext,
      )
    }

    Function("getArmedAndroidLiveUpdateGeneration") {
      AndroidLiveUpdateNotifications.currentGeneration(
        requireNotNull(appContext.reactContext).applicationContext,
      )
    }

    Function("armAndroidLiveUpdate") { generationId: String, seedJson: String ->
      AndroidLiveUpdateNotifications.arm(
        requireNotNull(appContext.reactContext).applicationContext,
        generationId,
        seedJson,
      )
    }

    Function("dismissAndroidLiveUpdate") {
      AndroidLiveUpdateNotifications.dismiss(
        requireNotNull(appContext.reactContext).applicationContext,
      )
    }

    Function("openAndroidLiveUpdateSettings") {
      AndroidLiveUpdateNotifications.openSettings(
        requireNotNull(appContext.reactContext).applicationContext,
      )
    }

    Function("getShowcasePairingUrl") {
      appContext.currentActivity?.intent?.getStringExtra("showcasePairingUrl")
    }

    Function("getShowcaseScene") {
      val storedScene = appContext.reactContext
        ?.filesDir
        ?.resolve("t3-showcase-scene")
        ?.takeIf { it.isFile }
        ?.readText()
        ?.trim()
        ?.takeIf { it.isNotEmpty() }
      storedScene ?: appContext.currentActivity?.intent?.getStringExtra("showcaseScene")
    }

    // The palette is fixed for the whole capture, so it only ever arrives as a
    // launch extra — unlike the scene, which the runner rewrites in place.
    Function("getShowcaseTheme") {
      appContext.currentActivity?.intent?.getStringExtra("showcaseTheme")
    }

    Function("prepareShowcaseCapture") {
      // Android app data is cleared by the host runner before launch.
    }

    Function("markShowcaseReady") { scene: String ->
      appContext.reactContext
        ?.filesDir
        ?.resolve("t3-showcase-ready")
        ?.writeText(scene)
    }
  }
}
