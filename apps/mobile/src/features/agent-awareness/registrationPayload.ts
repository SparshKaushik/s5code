import type { RelayDeviceRegistrationRequest } from "@t3tools/contracts/relay";

import type { Preferences } from "../../persistence/mobile-preferences";
import { supportsAndroidLiveUpdates } from "./androidLiveUpdates";
import { supportsAgentAwarenessPush } from "./capabilities";

// Development builds are Xcode-signed and receive sandbox APNs tokens;
// preview and production builds are distribution-signed and use production
// APNs. The relay routes each device's pushes accordingly. Android has no
// equivalent split: every build variant sends through the same FCM project.
export function resolveApsEnvironment(appVariant: unknown): "sandbox" | "production" {
  return appVariant === "development" ? "sandbox" : "production";
}

interface IosRegistrationInput {
  readonly platform: "ios";
  readonly deviceId: string;
  readonly label: string;
  readonly iosMajorVersion: number;
  readonly appVersion?: string;
  readonly bundleId?: string;
  readonly apsEnvironment?: "sandbox" | "production";
  readonly pushToken?: string;
  readonly pushToStartToken?: string;
  readonly notificationsEnabled: boolean;
  readonly preferences: Preferences;
}

interface AndroidRegistrationInput {
  readonly platform: "android";
  readonly deviceId: string;
  readonly label: string;
  readonly appVersion?: string;
  readonly fcmToken?: string;
  readonly notificationsEnabled: boolean;
  readonly preferences: Preferences;
}

export type RegistrationInput = IosRegistrationInput | AndroidRegistrationInput;

export function makeRelayDeviceRegistrationRequest(
  input: RegistrationInput,
): RelayDeviceRegistrationRequest {
  const pushAvailable = supportsAgentAwarenessPush();
  const liveActivitiesEnabled =
    pushAvailable &&
    ((input.platform === "ios" && input.preferences.liveActivitiesEnabled !== false) ||
      (input.platform === "android" &&
        supportsAndroidLiveUpdates() &&
        input.preferences.androidLiveUpdatesEnabled !== false));
  const preferences = {
    liveActivitiesEnabled,
    notificationsEnabled: pushAvailable && input.notificationsEnabled,
    notifyOnApproval: true,
    notifyOnInput: true,
    notifyOnCompletion: true,
    notifyOnFailure: true,
  };

  if (input.platform === "android") {
    return {
      deviceId: input.deviceId,
      label: input.label,
      platform: "android",
      appVersion: input.appVersion,
      ...(input.fcmToken ? { fcmToken: input.fcmToken } : {}),
      preferences,
    };
  }

  return {
    deviceId: input.deviceId,
    label: input.label,
    platform: "ios",
    iosMajorVersion: input.iosMajorVersion,
    appVersion: input.appVersion,
    ...(input.bundleId ? { bundleId: input.bundleId } : {}),
    ...(input.apsEnvironment ? { apsEnvironment: input.apsEnvironment } : {}),
    ...(input.pushToken ? { pushToken: input.pushToken } : {}),
    ...(input.pushToStartToken ? { pushToStartToken: input.pushToStartToken } : {}),
    preferences,
  };
}
