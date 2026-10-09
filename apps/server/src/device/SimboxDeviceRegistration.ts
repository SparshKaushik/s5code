import type { OrchestrationV2TurnItem } from "@t3tools/contracts";
import * as Context from "effect/Context";
import * as Effect from "effect/Effect";
import * as Clock from "effect/Clock";
import * as FileSystem from "effect/FileSystem";
import * as Layer from "effect/Layer";
import * as Path from "effect/Path";
import * as Schema from "effect/Schema";
import * as Semaphore from "effect/Semaphore";
import * as DeviceService from "./DeviceService.ts";
import { Registration } from "./SimboxDeviceHost.ts";

const receiptPattern =
  /\[simbox-device:([a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12})\]/g;

/** Read only tool output, never prompts, tool arguments, or assistant prose. */
export function receiptsForItem(
  item: Pick<OrchestrationV2TurnItem, "type"> & { readonly output?: unknown },
): string[] {
  if (item.type !== "command_execution" && item.type !== "dynamic_tool") return [];
  const ids = new Set<string>();
  const visit = (value: unknown, depth = 0) => {
    if (typeof value === "string") {
      for (const match of value.slice(-2_000_000).matchAll(receiptPattern)) ids.add(match[1]!);
    } else if (depth < 4 && value && typeof value === "object") {
      for (const child of Object.values(value).slice(0, 128)) visit(child, depth + 1);
    }
  };
  visit(item.output);
  return [...ids].slice(0, 16);
}

export class SimboxDeviceRegistrationError extends Schema.TaggedError<SimboxDeviceRegistrationError>()(
  "SimboxDeviceRegistrationError",
  {
    step: Schema.String,
    cause: Schema.Defect(),
  },
) {
  override get message(): string {
    return `Simbox device attachment failed while ${this.step}.`;
  }
}

export class SimboxDeviceRegistration extends Context.Service<
  SimboxDeviceRegistration,
  {
    readonly observe: (
      item: OrchestrationV2TurnItem,
    ) => Effect.Effect<void, SimboxDeviceRegistrationError>;
  }
>()("t3/device/SimboxDeviceRegistration") {}

const make = Effect.gen(function* () {
  const fs = yield* FileSystem.FileSystem;
  const path = yield* Path.Path;
  const devices = yield* DeviceService.DeviceService;
  const lock = yield* Semaphore.make(1);
  const directory =
    process.env.SIMBOX_DEVICE_REGISTRATION_DIR ??
    path.join(
      process.env.XDG_CONFIG_HOME ??
        path.join(process.env.HOME ?? process.env.USERPROFILE ?? "", ".config"),
      "simbox",
      "device-registrations",
    );
  const seen = new Set<string>();
  const decode = Schema.decodeUnknownEffect(Schema.fromJsonString(Registration));
  const observe: SimboxDeviceRegistration["Service"]["observe"] = (item) =>
    Effect.gen(function* () {
      for (const id of receiptsForItem(item)) {
        const input = yield* lock
          .withPermit(
            Effect.gen(function* () {
              if (seen.has(id)) return null;
              seen.add(id);
              if (seen.size > 4096) seen.delete(seen.values().next().value!);
              const file = path.join(directory, `${id}.json`);
              // The file must be a fresh CLI receipt; arbitrary output is insufficient.
              const exists = yield* fs.exists(file);
              if (!exists) return null;
              const info = yield* fs.stat(file);
              if (Number(info.size) > 16_384) return null;
              const claimed = path.join(directory, `${id}.claimed`);
              yield* fs.rename(file, claimed);
              return yield* Effect.gen(function* () {
                const input = yield* decode(yield* fs.readFileString(claimed));
                const age = (yield* Clock.currentTimeMillis) - input.createdAt;
                if (age < -10_000 || age > 5 * 60_000) return null;
                return input;
              }).pipe(Effect.ensuring(fs.remove(claimed, { force: true }).pipe(Effect.ignore)));
            }),
          )
          .pipe(
            Effect.mapError(
              (cause) =>
                new SimboxDeviceRegistrationError({ step: "reading the CLI receipt", cause }),
            ),
          );
        if (input)
          yield* devices.registerSimbox(item.threadId, input).pipe(
            Effect.asVoid,
            Effect.mapError(
              (cause) =>
                new SimboxDeviceRegistrationError({ step: "registering the runner", cause }),
            ),
          );
      }
    });
  return SimboxDeviceRegistration.of({ observe });
});

export const layer = Layer.effect(SimboxDeviceRegistration, make);
