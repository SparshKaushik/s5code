# Plan: Android Live Updates for Agent Activity

## Summary

Add an Android agent-activity notification that is updated in place while agent work is active. On Android 16+ it requests Live Update promotion and uses `NotificationCompat.ProgressStyle`; when the active thread exposes plan progress, each plan task is represented as a progress point (dot). Older Android versions receive the same lifecycle as a standard ongoing notification.

The notification remains remote-ready: the environment publishes canonical agent activity to T3 Connect, the relay sends a compact FCM data message, and native Android code renders it without starting React Native in the background.

This follows the already-shipped Android FCM registration and alert pipeline. It is the follow-up to `.plans/21-android-fcm-notifications.md`, whose FCM phase is now present in the repository.

## Product and platform decisions

1. **One aggregate surface per Android device.** Use one stable Android notification tag/id for all active agent work, matching the single aggregate iOS Live Activity.
2. **Native receive/render path.** Handle live-update FCM data in a native `FirebaseMessagingService`; do not depend on a cold JS/TaskManager launch. FCM gives `onMessageReceived` only a short execution window, and rendering requires no network request.
3. **Do not replace normal alerts.** Approval, input, completion, and failure still alert according to existing preferences. The live surface is normally silent and updated with `setOnlyAlertOnce(true)`; transition payloads may additionally create a normal alert notification using the existing high-importance alert channel.
4. **Progress dots from normalized plan tasks.** Use the existing `OrchestrationThreadShell.planProgress` (`step`, `completedSteps`, `totalSteps`). No provider-specific payload parsing belongs in Android or the relay.
5. **Progressive enhancement.** API 36+ uses `ProgressStyle` and requests promoted-ongoing treatment. API 35 and below use standard ongoing notification content. Lack of promotion permission on API 36 demotes presentation but does not disable updates.
6. **Foreground/local arming first.** Arm a generation when the user starts work from Android, and reconcile an already armed generation whenever the app foregrounds. Do not remotely create an unarmed Live Update on a closed app. This avoids unsolicited promoted notifications and follows Android's user-initiated Live Update guidance.
7. **Respect dismissal.** A user dismissal suppresses reposting for the same generation. A new explicit arm creates a new generation and may display again.
8. **No custom `RemoteViews`.** Live Updates do not support them. Use system templates only.

## Current baseline

Already available:

- Android FCM token registration and normal notification delivery.
- Relay aggregate state, freshness checks, signed queue jobs, token-current checks, dedupe, and permanent token invalidation.
- `OrchestrationThreadShell.planProgress`, populated from provider `turn.plan.updated` events. Pi todo tools (`todowrite`, `patchtodo`, `read_todo`) already normalize into this event shape; other adapters use the same canonical plan event.
- Android compile/target SDK 36 through Expo 56 / React Native 0.85.
- Deep-link routing from `environmentId`, `threadId`, and sanitized relative `deepLink`.

Missing:

- Plan progress is not copied into relay agent-awareness state.
- `thread.activity-appended` currently does not publish `turn.plan.updated` activity to the relay.
- Android registrations force `liveActivitiesEnabled` to false.
- FCM supports alert notifications only, not data-only live-update events.
- No native Android renderer, stable ongoing notification, promotion permission, or dismissal handling exists.

## Desired lifecycle

```text
Provider plan/tool event
  -> canonical turn.plan.updated
  -> ThreadPlanProgressService updates thread shell
  -> AgentAwarenessRelay publishes state (including optional planProgress)
  -> relay builds aggregate and chooses Android live update
  -> signed queue job
  -> FCM high-priority data-only message
  -> T3 FirebaseMessagingService
  -> NotificationManager.notify(stable tag/id)
```

Lifecycle events:

- **Arm:** Android app creates a random `generationId`, posts a seeded ongoing notification, and registers the generation with the relay.
- **Replay:** registration causes the relay to replay the authoritative current aggregate. A short freshly-armed grace period keeps the seed from being immediately canceled before the environment's first publish.
- **Update:** relay sends aggregate changes. Plan changes bypass the ordinary 15-second cosmetic update throttle so a final task transition cannot be lost without a trailing event.
- **Terminal:** the surface shows Done/Failed for the existing terminal display TTL and emits a normal alert when allowed.
- **End:** when the aggregate becomes null, relay sends an end event and native Android cancels the stable notification.
- **Foreground reconciliation:** if a generation is armed, fetch the existing agent-activity snapshot and render it locally or re-register the generation to trigger replay.
- **Disable/sign-out:** cancel locally, clear the armed generation, and tell the relay not to send further live updates.

## Phase 1 — Carry plan progress through agent awareness

### Contracts

In `packages/contracts/src/relay.ts`:

- Add an optional plan progress structure:

  ```ts
  {
    currentStep: string;
    completedSteps: number;
    totalSteps: number;
  }
  ```

- Add it as optional `planProgress` on both `RelayAgentActivityState` and `RelayAgentActivityAggregateRow`.
- Validate `totalSteps > 0` and `0 <= completedSteps <= totalSteps`.
- Keep the fields optional so older servers, relays, iOS builds, and cached payloads continue to decode.

In `packages/shared/src/agentAwareness.ts`:

- Include `planProgress` in `ProjectThreadAwarenessInput`.
- Copy and sanitize it into the awareness state only while present.
- Prefer the current plan step as running detail/headline input, but keep provider names and existing phase semantics intact.

In `infra/relay/src/agentActivity/AgentActivityPublisher.ts` and `agentActivityPayloads.ts`:

- Copy plan progress into aggregate rows.
- Truncate `currentStep` to the same short summary limit used for notification content.
- Include plan progress in aggregate identity and stale-state validation naturally through the existing aggregate schema.

### Publishing trigger

In `apps/server/src/relay/AgentAwarenessRelay.ts`:

- Allow `thread.activity-appended` with `activity.kind === "turn.plan.updated"` to publish.
- Keep proposed-plan markdown events excluded; only canonical execution-task progress belongs on the live surface.
- Add focused tests proving a Pi todo update and a generic provider plan update both result in awareness publication.

### Update throttling

In `infra/relay/src/agentActivity/Deliveries.ts`:

- Treat a change to the selected row's plan progress as a significant update, like attention, active-count, and terminal transitions.
- Bypass the 15-second cosmetic throttle for these changes. Plan tools update at task boundaries, not token frequency, so this remains low volume.

## Phase 2 — Add an Android live-update registration

Do not overload the iOS activity push-token request. Add an explicit backward-compatible endpoint:

```ts
POST /v1/mobile/live-updates
{
  deviceId: string;
  generationId: string;
}
```

Changes:

- Add `RelayAndroidLiveUpdateRegistrationRequest` and endpoint to `packages/contracts/src/relay.ts` and the managed relay client.
- Require an authenticated Android device owned by the current cloud user and a current FCM token.
- Add nullable `android_generation_id` and `android_armed_at` columns to `relay_live_activities` (or rename the table/service to activity surfaces only if that can be done without broad churn).
- Registration replaces the prior generation, clears its delivery baseline, marks it armed, and invokes the existing replay behavior.
- Device unregistration, sign-out preference changes, and permanent FCM token invalidation clear the Android generation.
- Keep dismissal local; a background broadcast receiver should not perform network I/O.

A generation id is required for ordering and dismissal semantics. It prevents a delayed FCM event from an old task run from recreating or overwriting a newer notification.

## Phase 3 — Extend relay delivery and FCM payloads

### Delivery kinds and signed jobs

Add explicit kinds rather than pretending Android notifications are ActivityKit activities:

- `live_update_update`
- `live_update_end`

Extend `RelayDeliveryKind`, queue job schemas, delivery attempts, logs, and tests. Add a nullable Android live-update payload to signed jobs; preserve signature, expiry, claim, stale aggregate, and current-token checks.

### Compact FCM data payload

FCM data values must be strings and the total payload is limited to 4096 bytes. Do not send the full five-row aggregate. Serialize one versioned compact JSON object under a `liveUpdate` data key containing:

```json
{
  "type": "agent_activity_live_update",
  "version": 1,
  "event": "update",
  "generationId": "...",
  "eventAt": "2026-...",
  "title": "T3 Code",
  "subtitle": "Agent work in progress",
  "activeCount": 2,
  "primary": {
    "environmentId": "...",
    "threadId": "...",
    "projectTitle": "...",
    "threadTitle": "...",
    "phase": "running",
    "status": "Working",
    "deepLink": "/threads/.../...",
    "planProgress": {
      "currentStep": "Implement native renderer",
      "completedSteps": 1,
      "totalSteps": 4
    }
  },
  "alert": null
}
```

Payload rules:

- Select `primary` using the same priority as the iOS surface: attention, failure, active work, then terminal/stale.
- Keep `activeCount` so the text can disclose additional agents without sending all rows.
- Sanitize and truncate before signing/queueing; assert encoded size below the FCM limit in tests.
- Use `android.priority = HIGH`, because the result is immediately user-visible.
- Use a stable FCM collapse key per agent-activity surface so an offline device receives the newest update/end rather than every intermediate state.
- Include `eventAt`; native code ignores older events within a generation. A generation mismatch is always ignored.
- An `end` payload needs only version, event, generation, and event time.

### Alert behavior

Reuse the existing attention/terminal transition calculations and preferences:

- If the Android surface is armed, put the transition alert in the live-update data payload. Native code updates the ongoing notification and posts a separate normal alert notification on the high-importance `agent-awareness` channel.
- If the surface is not armed, continue using the existing normal FCM notification request unchanged.
- Never alert on baseline-less replay, stale terminal state, or cosmetic plan progress.

### Persistence

A successful live-update delivery updates `lastAggregateJson` and `lastLiveActivityDeliveryAt`, just as iOS does. This keeps transition alerts, throttling, and replay behavior deterministic. An end marks the surface ended but retains enough generation metadata to reject delayed events.

## Phase 4 — Native Android module and FCM interception

Create a dedicated local Expo module, for example:

```text
apps/mobile/modules/t3-live-updates/
  expo-module.config.json
  android/build.gradle
  android/src/main/AndroidManifest.xml
  android/src/main/java/expo/modules/t3liveupdates/
    T3LiveUpdatesModule.kt
    T3FirebaseMessagingService.kt
    AgentLiveUpdateRenderer.kt
    AgentLiveUpdatePayload.kt
    AgentLiveUpdateDismissReceiver.kt
```

### Dependencies and manifest

- Pin `androidx.core:core-ktx:1.17.0`, which adds `NotificationCompat.ProgressStyle` and `setRequestPromotedOngoing`.
- Add non-runtime `android.permission.POST_PROMOTED_NOTIFICATIONS`.
- Keep the existing runtime `POST_NOTIFICATIONS` flow.
- Replace Expo's FCM service with a subclass of `ExpoFirebaseMessagingService` rather than registering two competing `com.google.firebase.MESSAGING_EVENT` services:
  - live-update data messages are consumed by T3 native code;
  - all other messages and token callbacks call `super`, preserving `expo-notifications` behavior.
- Use manifest merger rules to remove Expo's service declaration and register the T3 subclass once.
- Verify the merged manifest in prebuild output. This coupling to an Expo class must have a build test because Expo upgrades can move it.

This is a native fingerprint change and requires new Android binaries; it cannot ship as an OTA-only update.

### Native JS API

Expose a small API:

- `getCapability()` -> API support, notification permission, promotion eligibility, armed generation.
- `arm(generationId, seedPayload)` -> persist generation and post/update the seed.
- `update(payload)` -> foreground reconciliation path using the same renderer as FCM.
- `end(generationId?)` -> cancel and clear.
- `openPromotionSettings()` -> API 36 promotion settings, with app-notification settings fallback.

The module should no-op safely on non-Android platforms through a TypeScript wrapper.

### Notification construction

Use a dedicated channel such as `agent-live-updates`:

- Importance `LOW` or `DEFAULT`, never `MIN` (Live Update eligibility requirement).
- Stable notification tag/id.
- `setSmallIcon` using the existing monochrome notification asset.
- `setContentTitle`, concise content text, valid deep-link `PendingIntent` (`FLAG_IMMUTABLE | FLAG_UPDATE_CURRENT`).
- `setOngoing(true)`, `setOnlyAlertOnce(true)`, `setAutoCancel(false)`.
- `setRequestPromotedOngoing(true)`.
- No custom content view, group summary, or colorized notification.

On API 36+, expose `NotificationManager.canPostPromotedNotifications()` as capability only. Promotion is controlled by the OS/user/OEM; failure to promote is a valid standard-notification fallback, not an app error.

### Plan progress dots

When `planProgress` is valid:

- Use `NotificationCompat.ProgressStyle` on API 36+.
- Represent each task as a `ProgressStyle.Point` along a neutral segment.
- Completed task points use the completed tint, the current task point uses the active tint, and future points use a muted tint.
- Set the tracker/progress position to the current task boundary.
- Show `completedSteps/totalSteps` and the truncated current step in text so progress remains understandable with accessibility services and when dots are crowded.
- For one task, use one point at position zero and a minimum segment length of one.
- Cap visual points at a documented UI limit (recommended: 12). Above the limit, sample milestones while preserving first/current/last and always show the exact numeric count in text.
- If no plan is available, use a standard/BigText style with phase and thread information. On API <36, `ProgressStyle` falls back, but explicitly use standard progress/text to make the fallback predictable.

### Dismissal and ordering

- Attach a `deleteIntent` to `AgentLiveUpdateDismissReceiver`.
- Persist `dismissedGenerationId` in `SharedPreferences`.
- Ignore subsequent updates for that generation after dismissal; do not repost it.
- `arm` with a new generation clears dismissal suppression.
- Persist the newest accepted `eventAt`; ignore out-of-order messages.
- `end` always cancels matching current generation and clears active state.
- Never log FCM tokens, task text, or full payloads in production.

## Phase 5 — Mobile lifecycle and settings

In `apps/mobile/src/features/agent-awareness/remoteRegistration.ts`:

- Generalize the current arming call: iOS starts ActivityKit; Android calls the native live-update module, creates a generation id, and registers it with the relay.
- On foreground, reconcile only an already armed Android generation. Fetch the existing aggregate snapshot and feed it to native code, or re-register the generation to trigger replay.
- On sign-out and device unregistration, cancel the Android notification and clear local generation state.
- When the FCM token rotates, preserve the local generation but re-register the device and live-update generation after the new token is accepted.

In `registrationPayload.ts`:

- Allow Android `liveActivitiesEnabled` when the native module is present and the local preference is enabled.
- Do not claim support in an old binary where the module is absent.

In Settings:

- Show the second switch on Android as **Live Updates**; retain **Live Activity Updates** on iOS.
- Enabling requires normal notification permission, cloud sign-in, linked environments, and successful device registration.
- Disabling updates relay/environment preferences and immediately cancels the local surface.
- On API 36, if updates are enabled but promotion is disabled, show a separate explanatory row/action to open promoted-notification settings. Do not make the main switch lie: standard ongoing updates still work.
- Update sign-in/error copy to say “Live Updates” on Android rather than “Live Activities.”

Entry points:

- Existing task starts in `NewTaskDraftScreen.tsx` and `ThreadComposer.tsx` already call the shared arming function; preserve both.
- No new command-palette or keybinding entry applies on mobile.

## Provider and surface matrix

- **Pi:** todo tool results already become canonical plan steps; progress dots work without adapter changes.
- **Codex, Claude, Cursor, Grok, OpenCode:** use dots whenever the adapter emits `turn.plan.updated`; otherwise show the normal phase-only surface. Do not invent progress from tool logs.
- **Android:** new ongoing/promoted notification behavior.
- **iOS:** optional contract fields are ignored unless a later design chooses to render plan progress; existing Live Activity behavior remains unchanged.
- **Web/desktop:** no UI change. Their server must publish the optional plan progress through the relay, which is covered by shared server behavior.
- **Local-only environments:** no remote Live Updates, same as current Live Activities; T3 Connect/relay linkage remains required.
- **Remote/relay/tunnel:** transport is relay FCM and does not bake in client origins.

## Validation

### Pure/unit tests

- `packages/shared`: awareness projection includes valid optional plan progress and omits invalid/absent progress.
- `apps/server`: `turn.plan.updated` activity triggers relay publication; proposed-plan markdown still does not.
- `ThreadPlanProgress`: pending/current/completed counts, all-complete clearing, and stale-turn isolation.
- `packages/contracts`: old payload compatibility and new progress/registration schema validation.
- `infra/relay`:
  - Android registration ownership and generation replacement.
  - update/end choice, freshly-armed grace, disabled preference, no FCM token, and unarmed fallback.
  - plan changes bypass throttling.
  - transition alerts remain deduped/freshness-gated.
  - signed job verification, stale generation/token skip, permanent token invalidation.
  - FCM request is data-only, high priority, collapsible, and below 4096 bytes.
- Kotlin/JVM tests:
  - strict payload parsing/version rejection.
  - hero text and deep-link validation.
  - point positions/colors for 1, partial, complete, absent, and >12 tasks.
  - stale timestamp/generation rejection.
  - dismissal suppression and new-generation reset.
  - update/end use one stable notification id.

### Build checks

- Targeted mobile TypeScript typecheck.
- Android prebuild and merged-manifest assertion: one FCM messaging service, promoted permission present.
- Build API 36 with pinned AndroidX Core 1.17.0.
- Relay targeted tests and typecheck for changed packages only; no repo-wide checks.

### Integrated Android pass

After implementation, request permission before launching an emulator/browser workflow and use `test-t3-mobile` once on the integrated result:

1. API 36 Google Play emulator, app foreground/background/terminated.
2. Promotion allowed: chip/lock-screen/shade placement appears when OEM/system qualifies it.
3. Promotion disabled: standard ongoing notification still updates.
4. API 35 emulator: standard ongoing fallback works without crashes.
5. Plan with 4 tasks: dots advance after each task result; current-step text changes.
6. No-plan provider: phase-only notification.
7. Approval/input: live surface updates and exactly one normal alert appears.
8. Completion/failure: terminal content and one preference-gated alert; surface later ends.
9. Tap routes to the correct environment/thread.
10. Swipe/dismiss (where system permits): same generation does not return; a new local task can arm a new one.
11. Out-of-order and duplicate FCM payloads do not regress content.
12. Token rotation and app restart reconcile the armed generation.
13. Multiple environments/agents show the selected primary row and accurate aggregate count.

## Documentation

- Add user documentation under `docs/user/` covering Android version behavior, notification permission, optional promoted presentation, Live Updates setting, and dismissal.
- Add internal documentation under `docs/internals/` for the generation lifecycle, native FCM interception, compact payload, and plan-progress propagation.
- Update relay operations docs only if new delivery kinds/metrics or rollout controls require operator action.

## Rollout and observability

- Gate relay delivery on a successful Android live-update registration, not only `platform === "android"`, so old binaries never receive unknown data messages.
- Record delivery kind/channel, queued/success/failure, payload version, and promotion capability reported by the app; do not record task text or tokens.
- Track live-update FCM permanent failures separately from normal alert failures even though both invalidate the same current token.
- Roll out development -> preview -> production. Validate on stock Android 16 first, then at least one OEM because OEMs may impose additional promotion criteria.

## Main risks

- **Android eligibility/policy:** agent work must remain finite, user-initiated, and useful to monitor. Do not turn this into an ambient “agents exist” surface or auto-start it for unrelated remote events.
- **Expo FCM ownership:** Android dispatches one messaging service. Registering a second service without replacing/forwarding Expo's service can break normal notifications and token rotation.
- **Background delivery:** high-priority FCM is not guaranteed. Foreground replay and generation/timestamp checks repair drift without polling.
- **Payload size:** full aggregates can exceed FCM's 4096-byte limit; the compact primary-row payload is required.
- **Alert duplication:** live visual updates and normal alerts must share one transition decision, or waiting/completion events can buzz twice.
- **Dismissal loops:** reposting a dismissed generation violates platform guidance and user intent.
- **Native release boundary:** manifest, service, permission, and AndroidX changes require a new binary and correct Expo fingerprinting.

## Suggested implementation slices

1. Optional plan-progress contracts, awareness projection, publish trigger, and tests.
2. Android live-update registration contract/persistence/replay.
3. Signed delivery kinds and compact FCM data request, with relay tests.
4. Native module, service forwarding, renderer, dismissal/order tests.
5. Mobile arming/reconciliation/settings integration.
6. Targeted builds, integrated Android validation, docs, then staged rollout.

Each slice should remain independently testable; do not combine the native service replacement with unrelated mobile notification refactors.
