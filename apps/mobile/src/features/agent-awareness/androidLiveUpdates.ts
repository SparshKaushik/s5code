import * as Crypto from "expo-crypto";
import { requireOptionalNativeModule } from "expo-modules-core";
import { Platform } from "react-native";

export interface AndroidLiveUpdateSeed {
  readonly threadTitle: string;
  readonly projectTitle: string;
}

interface NativeAndroidLiveUpdates {
  readonly canPostAndroidLiveUpdates: () => boolean;
  readonly getArmedAndroidLiveUpdateGeneration: () => string | null;
  readonly armAndroidLiveUpdate: (generationId: string, seedJson: string) => void;
  readonly dismissAndroidLiveUpdate: () => void;
  readonly openAndroidLiveUpdateSettings: () => void;
}

function nativeModule(): NativeAndroidLiveUpdates | null {
  if (Platform.OS !== "android") return null;
  return requireOptionalNativeModule<NativeAndroidLiveUpdates>("T3NativeControls");
}

export function supportsAndroidLiveUpdates(): boolean {
  return Platform.OS === "android" && Number(Platform.Version) >= 36 && nativeModule() !== null;
}

export function canPostAndroidLiveUpdates(): boolean {
  return nativeModule()?.canPostAndroidLiveUpdates() ?? false;
}

export function getArmedAndroidLiveUpdateGeneration(): string | null {
  return nativeModule()?.getArmedAndroidLiveUpdateGeneration() ?? null;
}

export function ensureAndroidLiveUpdateGeneration(): string | null {
  const module = nativeModule();
  if (!module) return null;
  const existing = module.getArmedAndroidLiveUpdateGeneration();
  if (existing) return existing;
  const generationId = Crypto.randomUUID();
  module.armAndroidLiveUpdate(
    generationId,
    JSON.stringify({ threadTitle: "Agent work", projectTitle: "S5 Code" }),
  );
  return generationId;
}

export function armAndroidLiveUpdate(generationId: string, seed: AndroidLiveUpdateSeed): void {
  nativeModule()?.armAndroidLiveUpdate(generationId, JSON.stringify(seed));
}

export function dismissAndroidLiveUpdate(): void {
  nativeModule()?.dismissAndroidLiveUpdate();
}

export function openAndroidLiveUpdateSettings(): void {
  nativeModule()?.openAndroidLiveUpdateSettings();
}
