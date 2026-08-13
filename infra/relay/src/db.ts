import type { PgClient } from "@effect/sql-pg/PgClient";
import * as Cloudflare from "alchemy/Cloudflare";
import type { PublicOrigin } from "alchemy/Cloudflare/Hyperdrive";
import * as Config from "effect/Config";
import type { EffectPgDatabase } from "drizzle-orm/effect-postgres";
import * as Context from "effect/Context";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Redacted from "effect/Redacted";

export class RelayDb extends Context.Service<
  RelayDb,
  EffectPgDatabase & {
    readonly $client: PgClient;
  }
>()("t3code-relay/db/RelayDb") {}

export class RelayTransactions extends Context.Service<
  RelayTransactions,
  {
    readonly withTransaction: RelayDb["Service"]["$client"]["withTransaction"];
  }
>()("t3code-relay/db/RelayTransactions") {
  static readonly layer = Layer.effect(
    RelayTransactions,
    Effect.gen(function* () {
      const db = yield* RelayDb;
      return RelayTransactions.of({
        withTransaction: db.$client.withTransaction,
      });
    }),
  );
}

/**
 * Paths must be kept in sync with the migrations generated into
 * `./migrations/postgres`. Migrations are applied against the Neon database
 * out-of-band from the relay deploy (see the relay README) — Alchemy does not
 * run them for a Neon origin.
 */

/**
 * Parse a Postgres connection URL (for example a Neon pooled connection
 * string) into the origin `Cloudflare.Hyperdrive` connects to. Hyperdrive
 * provides its own pooled connection into Cloudflare's edge, so the Worker
 * talks to Neon through the Hyperdrive binding rather than over the public
 * internet.
 */
function parsePostgresUrl(rawUrl: string): PublicOrigin {
  const url = new URL(rawUrl);
  return {
    scheme: "postgres",
    host: url.hostname,
    ...(url.port ? { port: Number(url.port) } : {}),
    database: url.pathname.replace(/^\/+/, "") || "neondb",
    user: url.username ? decodeURIComponent(url.username) : "postgres",
    // Neon role passwords and per-branch tokens are frequently percent-encoded
    // in the connection string; decode before handing the password to
    // Hyperdrive, which ships it to Cloudflare's edge as-is.
    password: Redacted.make(url.password ? decodeURIComponent(url.password) : ""),
  };
}

export const RelayHyperdrive = Effect.gen(function* () {
  const databaseUrl = yield* Config.nonEmptyString("DATABASE_URL");
  return yield* Cloudflare.Hyperdrive.Connection("RelayHyperdrive", {
    origin: parsePostgresUrl(databaseUrl),
    caching: {
      disabled: true,
    },
    originConnectionLimit: 20,
  });
});
