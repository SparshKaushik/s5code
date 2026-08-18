# Android Live Updates internals

Android Live Updates reuse the agent-awareness publication path used by iOS Live Activities. `ThreadPlanProgressService` enriches the awareness state with the active plan step and completed/total counts; the relay aggregates and bounds that state before delivery.

The mobile client first registers an Android device with its FCM token. A local task start creates and persists a generation in the native module, posts a seed notification, then arms `/v1/mobile/android-live-updates` with `{ deviceId, generationId }`. Foreground reconciliation only re-registers a generation that is already armed. The relay stores that generation independently from ordinary alerts and includes it in signed queue jobs and compact FCM data payloads.

Relay jobs use kind `android_live_update`. FCM receives a data-only message with `t3Type=android_live_update` and versioned JSON under `liveUpdate`. The custom `ExpoFirebaseMessagingService` subclass consumes that type and forwards all other FCM traffic to Expo Notifications. Native code rejects generation mismatches and out-of-order timestamps, then updates or ends one stable notification. Android 16 uses `Notification.ProgressStyle`, requests promoted-ongoing treatment, and renders bounded plan-progress milestones. Standard alert messages continue through Expo Notifications.

The schema lives in `relay_android_live_updates`; apply relay migrations before deploying the worker. Android native code lives in the local `t3-native-controls` Expo module and therefore requires a new binary rather than an OTA-only release.
