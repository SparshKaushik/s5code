import type * as Effect from "effect/Effect";

/** Relay telemetry is disabled in S5 Code; these wrappers are no-op passthroughs. */
export const withRelayClientTracing = <A, E, R>(
  effect: Effect.Effect<A, E, R>,
): Effect.Effect<A, E, R> => effect;

export const withLocalTracing = <A, E, R>(effect: Effect.Effect<A, E, R>): Effect.Effect<A, E, R> =>
  effect;
