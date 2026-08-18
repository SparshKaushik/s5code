# Android Live Updates

S5 Code Mobile uses Android 16 Live Updates to keep active agent work visible on supported Android devices. The surface is separate from alert notifications: users can enable **Live Updates** in Settings, and approval, input, completion, and failure alerts continue to use the normal notification channel.

A Live Update is armed when the user starts agent work from Android or enables the setting. T3 Connect stores the armed device and sends compact high-priority FCM data messages as agent state changes. The notification shows the active thread, project, state, and plan step progress when the provider reports a plan. Opening it returns to the active thread.

Requirements:

- Android 16 / API 36 or newer
- Notification permission granted
- Live Updates enabled for S5 Code in Android system settings
- Signed in to T3 Connect with at least one linked environment

Turning Live Updates off dismisses the current ongoing notification and stops future state delivery to that surface. If Android promotion is disabled, the update remains available as a standard ongoing notification; Settings shows a separate action for promotion controls. Dismissing the notification suppresses the currently armed generation, and starting new local work arms a new one. Existing Android versions continue to receive standard alerts but do not show the Live Update switch as available.
