# Plan: Android FCM Notifications + Ongoing "Agent Activity" Notification

## Summary

Add Android push notifications to the relay, matching the existing iOS APNs
notifications (approval / input / completion / failure) with the same
preferences, dedupe, freshness, and deep-link semantics. Then add an Android
"live activity" analog as a persistent **ongoing notification** updated in
place via FCM data messages. Android 16 "Live Updates" (lock-screen/chip
promotion) is explicitly deferred to a follow-up.

## Decisions (locked)

1. **Live-activity scope**: ongoing notification now; Android 16 Live Updates
   (ProgressStyle + promoted ongoing) later.
2. **Delivery pipeline**: generalize the shared decision/queue/job machinery to
   be provider-neutral; keep `ApnsClient` and a new `FcmClient` as
   transport-specific.
3. **Firebase**: no Firebase project / FCM service account exists yet; setup is
   an explicit prerequisite task in this plan.

## Motivation

The relay already delivers iOS notifications + Live Activities over APNs. The
mobile app is iOS-only for agent awareness (`canRegisterRemoteLiveActivities()`
returns `Platform.OS === "ios"`). Android users get nothing. The four
notification events are transport-agnostic; only the bottom transport layer is
Apple-specific. FCM is the Android equivalent of APNs, and `expo-notifications`
is already push-service agnostic.

## Reference architecture (current APNs path)

```
AgentActivityPublisher.publish()
  └─ ApnsDeliveries.sendForTarget()          ← decision logic (choose delivery, throttle, alerts)
       └─ ApnsDeliveryQueue.enqueue*()        ← signed job → Cloudflare Queue
            └─ worker consumes queue → ApnsDeliveries.processSignedJob()
                 └─ ApnsClient.send*()         ← transport (HTTP/2 → api.push.apple.com)
```

The decision logic (`ApnsDeliveries.ts`) and the signed-job machinery
(`apnsDeliveryJobs.ts`, `DeliveryAttempts.ts`) are transport-agnostic. Only
`ApnsClient.ts` + the APNS-named fields are Apple-specific.

## What "same as APNs" means (must preserve)

- Four events: `waiting_for_approval`, `waiting_for_input`, `completed`,
  `failed`, each gated by `notifyOnApproval` / `notifyOnInput` /
  `notifyOnCompletion` / `notifyOnFailure` + master `notificationsEnabled`.
- Freshness window (`TERMINAL_NOTIFICATION_FRESHNESS_MS`, 2 min) so replays /
  restarts don't re-buzz.
- State-identity recheck (`notificationStateIsCurrent`) — drop a queued job if
  the thread's `phase`/`updatedAt` moved on.
- Token-current recheck + permanent-failure token invalidation.
- Signed, expiring delivery jobs (HMAC-signed, 10-min TTL) with dedupe via
  `DeliveryAttempts`.
- Deep-link routing via `environmentId`/`threadId`/`deepLink` in the payload.

---

## Phase 0 — Firebase / FCM setup (prerequisite)

1. Create a Firebase project; enable Cloud Messaging.
2. Add the Android app (package `club.touchtech.s5code` + dev/preview variants),
   download `google-services.json` (public, committable).
3. Generate a service-account private key (Firebase console → Project settings →
   Service accounts → "Generate new private key"). This JSON is the server-side
   credential the relay uses to mint FCM OAuth2 access tokens.
4. Add `google-services.json` to `apps/mobile/` and reference it in
   `app.config.ts` via `android.googleServicesFile`.
5. Document new env vars in `infra/relay/.env.example` and `infra/relay/README.md`.

---

## Phase 1 — FCM push notifications (the "same as APNs" ask)

### Contracts — `packages/contracts/src/relay.ts`

- `RelayAgentAwarenessPlatform`: `Literal("ios")` → `Literal("ios" | "android")`.
- `RelayDeviceRegistrationRequest`: add `fcmToken` (optional) and make
  `iosMajorVersion` optional / add an Android version field. Keep
  `bundleId`/`apsEnvironment` optional (iOS-only).
- `RelayClientDeviceRecord`: version field becomes platform-appropriate.
- `RelayDeliveryResult`: generalize `apnsStatus`/`apnsReason`/`apnsId` →
  provider-neutral (`deliveryStatus`/`deliveryReason`/`providerMessageId`), or
  add parallel `fcm*` fields. (See generalization note below.)

### Schema — `infra/relay/src/persistence/schema.ts`

- `relayMobileDevices.platform` `$type<"ios">` → `"ios" | "android"`; add
  `fcmToken` column (or rename `pushToken` → platform token).
- `relayDeliveryAttempts`: `apnsStatus`/`apnsReason`/`apnsId` → provider-neutral
  columns.
- New migration in `migrations/postgres/` (drizzle-kit, applied out-of-band).

### Config — `infra/relay/src/Config.ts` + `worker.ts`

- Add `fcm: { projectId, serviceAccountKey }` to `RelayConfiguration`, gated by
  an `fcmEnabled` flag mirroring `apnsEnabled` (optional; relay deploys without
  it).
- New env vars: `FCM_PROJECT_ID`, `FCM_SERVICE_ACCOUNT_KEY`.

### FCM transport — new `FcmClient.ts` + `fcmJwt.ts` (+ `FcmProviderTokens.ts`)

- Mirror `ApnsClient.ts` / `apnsJwt.ts` / `ApnsProviderTokens.ts`:
  - Sign a service-account JWT (RS256), exchange for an OAuth2 access token
    (scope `https://www.googleapis.com/auth/firebase.messaging`), cache ~45 min.
  - `POST https://fcm.googleapis.com/v1/projects/{projectId}/messages:send`.
  - Message shape:
    ```json
    { "message": {
        "token": "<fcm-token>",
        "notification": { "title": "...", "body": "..." },
        "data": { "environmentId": "...", "threadId": "...", "deepLink": "...",
                  "phase": "...", "updatedAt": "..." },
        "android": { "priority": "HIGH" }
    } }
    ```
  - Permanent-failure handling: `UNREGISTERED` / `INVALID_ARGUMENT` (bad token)
    → invalidate token, same as APNs `BadDeviceToken`.

### Delivery queue / jobs — generalize

- Rename `ApnsDeliveryQueue.ts` → `DeliveryQueue.ts`,
  `apnsDeliveryJobs.ts` → `deliveryJobs.ts`; add a `channel: "apns" | "fcm"`
  discriminator to the job payload. The signing/expiry/dedupe machinery is
  reusable as-is.
- `ApnsDeliveries.ts` → `Deliveries.ts` (or keep name, add FCM branch): the
  decision logic stays shared; only the final `send*` call branches to
  `ApnsClient` vs `FcmClient` based on the target's platform/token.

### Worker — `worker.ts`

- Add an `FcmDeliveryQueue` Cloudflare Queue binding + consumer, gated on
  `fcmEnabled`, mirroring the APNs consumer block.
- Add `FcmClient`/`FcmProviderTokens` layers to the runtime graph (always
  provided, like APNs, with placeholder creds when disabled).

### Mobile — `apps/mobile/`

- `registrationPayload.ts`: branch on platform — Android sends
  `platform: "android"`, `fcmToken`, Android app version (no
  `iosMajorVersion`/`bundleId`/`apsEnvironment`).
- `remoteRegistration.ts`:
  - Replace `canRegisterRemoteLiveActivities() = Platform.OS === "ios"` with a
    platform-aware capability check.
  - `nativePushTokenRegistration`: accept `{ type: "android", data }` from
    `getDevicePushTokenAsync()`; call `Notifications.setNotificationChannelAsync(...)`
    first (required on Android 13+).
  - `ensurePushTokenListener`: make the token-type check platform-agnostic.
- `notificationPayload.ts` / `notificationNavigation.ts` /
  `notificationResponseConsumer.ts`: already platform-agnostic (read
  `content.data`); work as-is once the FCM `data` payload carries
  `environmentId`/`threadId`/`deepLink`.

---

## Phase 2 — Ongoing "agent activity" notification (Android live-activity analog)

There is no true iOS Live Activity on Android. The universal surface is a
persistent **ongoing notification** with a stable ID, updated in place.

- **Arm locally**: when the user starts agent work from the app (foreground),
  post an ongoing notification (`Notifications.scheduleNotificationAsync` with
  a fixed `identifier`, `ongoing: true`), seeded with the current aggregate.
- **Report the ID**: register the notification's `identifier` with the relay
  (analogous to `activityPushToken` on iOS). Add a
  `registerAgentActivityNotification` endpoint + `RelayAgentActivityNotificationRegistrationRequest`
  contract, or fold it into the existing live-activity registration with a
  platform discriminator.
- **Update via FCM data messages**: the relay sends FCM `data`-only messages
  (carrying the aggregate) for `live_activity_update`/`live_activity_end`
  equivalents. The app handles them in a headless task
  (`Notifications.registerTaskAsync`, Android) and updates/removes the ongoing
  notification in place.
- **End**: on terminal aggregate / no live work, remove the ongoing
  notification (mirroring `live_activity_end`).

### Deferred (Phase 3, follow-up)

Android 16 "Live Updates" (`Notification.ProgressStyle`/`MetricStyle` +
`setRequestPromotedOngoing` + `POST_PROMOTED_NOTIFICATIONS` permission) as a
progressive enhancement behind a small local native Expo module
(`apps/mobile/modules/*` pattern). `expo-notifications` does not expose these
APIs, so this needs native code.

---

## Generalization note (decision #2)

Shared logic to rename / make provider-neutral:

- `ApnsDeliveries.ts` → shared decision logic + a `send*` branch to
  `ApnsClient` vs `FcmClient`.
- `ApnsDeliveryQueue.ts` → `DeliveryQueue.ts`.
- `apnsDeliveryJobs.ts` → `deliveryJobs.ts` with a `channel` field.
- `RelayDeliveryResult.apnsStatus/apnsReason/apnsId` → provider-neutral names.
- `relayDeliveryAttempts.apnsStatus/apnsReason/apnsId` → provider-neutral columns.

Keep transport-specific: `ApnsClient.ts` / `apnsJwt.ts` / `ApnsProviderTokens.ts`
and new `FcmClient.ts` / `fcmJwt.ts` / `FcmProviderTokens.ts`.

---

## Risks

- **Token semantics differ**: FCM tokens are per-app-install and rotate on app
  data clear / reinstall; the token-current recheck must use the FCM token, not
  the APNs token.
- **Android 13+ permission**: `POST_NOTIFICATIONS` runtime permission + channel
  creation must precede token fetch or registration silently no-ops.
- **Background data-message handling**: `registerTaskAsync` headless task has
  Android-specific lifecycle constraints (doze, app-standby). Ongoing-notification
  updates may be delayed in the background; acceptable for v1.
- **Generalization churn**: renaming APNS-named fields touches contracts,
  schema, worker, and all tests; do it in one focused PR to avoid drift.
- **Firebase setup is external**: blocked until the Firebase project +
  service-account key exist.

## Validation

- Backend: focused tests for `FcmClient` (token mint + send), the generalized
  delivery queue/jobs, and the shared decision logic (mirroring
  `ApnsDeliveries.test.ts` / `apnsDeliveryJobs.test.ts`).
- Contracts: schema round-trip tests for the new platform/registration fields.
- Mobile: `registrationPayload` / `remoteRegistration` unit tests for the
  Android branch; `test-t3-mobile` integrated pass for the ongoing notification
  (requires a real device/emulator with FCM).
- Manual: send a completion/approval event and verify the Android notification
  + deep-link navigation.

## Milestones

1. Firebase/FCM setup (Phase 0).
2. Contracts + schema + config generalization (Phase 1, first half).
3. `FcmClient` + queue/job generalization + worker wiring (Phase 1, second half).
4. Mobile token/permission/registration (Phase 1, third half).
5. Ongoing notification arm/update/end (Phase 2).
