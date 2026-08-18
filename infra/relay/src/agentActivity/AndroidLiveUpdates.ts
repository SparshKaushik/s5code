import {
  RelayAgentActivityAggregateState,
  type RelayAndroidLiveUpdateRegistrationRequest,
} from "@t3tools/contracts/relay";
import * as Context from "effect/Context";
import * as DateTime from "effect/DateTime";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Schema from "effect/Schema";
import { and, eq } from "drizzle-orm";

import * as RelayDb from "../db.ts";
import { relayAndroidLiveUpdates, relayMobileDevices } from "../persistence/schema.ts";

const encodeJsonValue = Schema.encodeEffect(Schema.fromJsonString(Schema.Unknown));

export class AndroidLiveUpdatePersistenceError extends Schema.TaggedErrorClass<AndroidLiveUpdatePersistenceError>()(
  "AndroidLiveUpdatePersistenceError",
  {
    operation: Schema.Literals(["register", "list-targets", "get-target", "mark-delivery"]),
    userId: Schema.String,
    deviceId: Schema.NullOr(Schema.String),
    cause: Schema.Defect(),
  },
) {
  override get message(): string {
    return `Failed to persist Android Live Update state during ${this.operation} for user ${this.userId}.`;
  }
}

export interface AndroidLiveUpdateTargetRow {
  readonly user_id: string;
  readonly device_id: string;
  readonly platform: "android";
  readonly fcm_token: string | null;
  readonly preferences_json: string;
  readonly generation_id: string;
  readonly armed_at: string;
  readonly last_aggregate_json: string | null;
  readonly last_delivery_at: string | null;
}

export class AndroidLiveUpdates extends Context.Service<
  AndroidLiveUpdates,
  {
    readonly register: (input: {
      readonly userId: string;
      readonly registration: RelayAndroidLiveUpdateRegistrationRequest;
    }) => Effect.Effect<void, AndroidLiveUpdatePersistenceError>;
    readonly listTargets: (input: {
      readonly userId: string;
    }) => Effect.Effect<
      ReadonlyArray<AndroidLiveUpdateTargetRow>,
      AndroidLiveUpdatePersistenceError
    >;
    readonly getTarget: (input: {
      readonly userId: string;
      readonly deviceId: string;
    }) => Effect.Effect<AndroidLiveUpdateTargetRow | null, AndroidLiveUpdatePersistenceError>;
    readonly markDelivery: (input: {
      readonly userId: string;
      readonly deviceId: string;
      readonly aggregate: RelayAgentActivityAggregateState | null;
      readonly deliveredAt: string;
    }) => Effect.Effect<void, AndroidLiveUpdatePersistenceError>;
  }
>()("t3code-relay/agentActivity/AndroidLiveUpdates") {}

export const make = Effect.gen(function* () {
  const db = yield* RelayDb.RelayDb;

  return AndroidLiveUpdates.of({
    register: Effect.fn("relay.android_live_updates.register")(function* (input) {
      const updatedAt = DateTime.formatIso(yield* DateTime.now);
      yield* db
        .insert(relayAndroidLiveUpdates)
        .values({
          userId: input.userId,
          deviceId: input.registration.deviceId,
          generationId: input.registration.generationId,
          armedAt: updatedAt,
          lastAggregateJson: null,
          lastDeliveryAt: null,
          createdAt: updatedAt,
          updatedAt,
        })
        .onConflictDoUpdate({
          target: [relayAndroidLiveUpdates.userId, relayAndroidLiveUpdates.deviceId],
          set: {
            generationId: input.registration.generationId,
            armedAt: updatedAt,
            lastAggregateJson: null,
            lastDeliveryAt: null,
            updatedAt,
          },
        })
        .pipe(
          Effect.mapError(
            (cause) =>
              new AndroidLiveUpdatePersistenceError({
                operation: "register",
                userId: input.userId,
                deviceId: input.registration.deviceId,
                cause,
              }),
          ),
        );
    }),
    listTargets: Effect.fn("relay.android_live_updates.list_targets")(function* (input) {
      return yield* db
        .select({
          user_id: relayMobileDevices.userId,
          device_id: relayMobileDevices.deviceId,
          platform: relayMobileDevices.platform,
          fcm_token: relayMobileDevices.fcmToken,
          preferences_json: relayMobileDevices.preferencesJson,
          generation_id: relayAndroidLiveUpdates.generationId,
          armed_at: relayAndroidLiveUpdates.armedAt,
          last_aggregate_json: relayAndroidLiveUpdates.lastAggregateJson,
          last_delivery_at: relayAndroidLiveUpdates.lastDeliveryAt,
        })
        .from(relayAndroidLiveUpdates)
        .innerJoin(
          relayMobileDevices,
          and(
            eq(relayMobileDevices.userId, relayAndroidLiveUpdates.userId),
            eq(relayMobileDevices.deviceId, relayAndroidLiveUpdates.deviceId),
          ),
        )
        .where(eq(relayAndroidLiveUpdates.userId, input.userId))
        .pipe(
          Effect.map((rows) =>
            rows
              .filter(
                (row): row is typeof row & { platform: "android"; fcm_token: string } =>
                  row.platform === "android" && row.fcm_token !== null,
              )
              .map((row) => ({
                ...row,
                preferences_json: JSON.stringify(row.preferences_json),
                last_aggregate_json:
                  row.last_aggregate_json === null ? null : JSON.stringify(row.last_aggregate_json),
              })),
          ),
          Effect.mapError(
            (cause) =>
              new AndroidLiveUpdatePersistenceError({
                operation: "list-targets",
                userId: input.userId,
                deviceId: null,
                cause,
              }),
          ),
        );
    }),
    getTarget: Effect.fn("relay.android_live_updates.get_target")(function* (input) {
      const rows = yield* db
        .select({
          user_id: relayMobileDevices.userId,
          device_id: relayMobileDevices.deviceId,
          platform: relayMobileDevices.platform,
          fcm_token: relayMobileDevices.fcmToken,
          preferences_json: relayMobileDevices.preferencesJson,
          generation_id: relayAndroidLiveUpdates.generationId,
          armed_at: relayAndroidLiveUpdates.armedAt,
          last_aggregate_json: relayAndroidLiveUpdates.lastAggregateJson,
          last_delivery_at: relayAndroidLiveUpdates.lastDeliveryAt,
        })
        .from(relayAndroidLiveUpdates)
        .innerJoin(
          relayMobileDevices,
          and(
            eq(relayMobileDevices.userId, relayAndroidLiveUpdates.userId),
            eq(relayMobileDevices.deviceId, relayAndroidLiveUpdates.deviceId),
          ),
        )
        .where(
          and(
            eq(relayAndroidLiveUpdates.userId, input.userId),
            eq(relayAndroidLiveUpdates.deviceId, input.deviceId),
          ),
        )
        .limit(1)
        .pipe(
          Effect.mapError(
            (cause) =>
              new AndroidLiveUpdatePersistenceError({
                operation: "get-target",
                userId: input.userId,
                deviceId: input.deviceId,
                cause,
              }),
          ),
        );
      const row = rows[0];
      if (!row || row.platform !== "android" || row.fcm_token === null) {
        return null;
      }
      const preferencesJson = yield* encodeJsonValue(row.preferences_json).pipe(Effect.orDie);
      const aggregateJson =
        row.last_aggregate_json === null
          ? null
          : yield* encodeJsonValue(row.last_aggregate_json).pipe(Effect.orDie);
      return {
        ...row,
        platform: "android" as const,
        preferences_json: preferencesJson,
        last_aggregate_json: aggregateJson,
      };
    }),
    markDelivery: Effect.fn("relay.android_live_updates.mark_delivery")(function* (input) {
      yield* db
        .update(relayAndroidLiveUpdates)
        .set({
          lastAggregateJson: input.aggregate,
          lastDeliveryAt: input.deliveredAt,
          updatedAt: input.deliveredAt,
        })
        .where(
          and(
            eq(relayAndroidLiveUpdates.userId, input.userId),
            eq(relayAndroidLiveUpdates.deviceId, input.deviceId),
          ),
        )
        .pipe(
          Effect.mapError(
            (cause) =>
              new AndroidLiveUpdatePersistenceError({
                operation: "mark-delivery",
                userId: input.userId,
                deviceId: input.deviceId,
                cause,
              }),
          ),
        );
    }),
  });
});

export const layer = Layer.effect(AndroidLiveUpdates, make);
