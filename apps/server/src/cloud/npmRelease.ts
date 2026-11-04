import {
  ServerNpmReleaseError,
  ServerNpmReleaseInput,
  type ServerNpmRelease,
} from "@t3tools/contracts";
import {
  cliReleaseIndexPageUrl,
  cliReleaseTagUrl,
  newestCliReleaseVersion,
} from "@t3tools/shared/cliRelease";
import * as Cache from "effect/Cache";
import * as Context from "effect/Context";
import * as Duration from "effect/Duration";
import * as Effect from "effect/Effect";
import * as Layer from "effect/Layer";
import * as Option from "effect/Option";
import * as Schema from "effect/Schema";
import { FetchHttpClient, HttpClient, HttpClientRequest, HttpClientResponse } from "effect/http";

const GitHubRelease = Schema.Struct({
  tag_name: Schema.String,
  draft: Schema.optional(Schema.Boolean),
});
const decodeRelease = Schema.decodeUnknownEffect(Schema.fromJsonString(GitHubRelease));
const decodeReleases = Schema.decodeUnknownEffect(
  Schema.fromJsonString(Schema.Array(GitHubRelease)),
);
const decodeInput = Schema.decodeUnknownEffect(ServerNpmReleaseInput);

export class NpmRelease extends Context.Service<
  NpmRelease,
  {
    readonly get: (
      input: ServerNpmReleaseInput,
    ) => Effect.Effect<ServerNpmRelease | null, ServerNpmReleaseError>;
  }
>()("t3/cloud/npmRelease") {}

/**
 * S5 Code servers install from the fork's GitHub Releases, not npm, where `t3` is upstream's
 * package. Those releases carry no migration manifest, so a track switch that needs one stays blocked.
 */
export const make = Effect.fn("NpmRelease.make")(function* () {
  const http = yield* HttpClient.HttpClient;
  const unreachable = new ServerNpmReleaseError({
    reason: "GitHub Releases could not be reached.",
  });
  const invalid = new ServerNpmReleaseError({
    reason: "GitHub returned invalid release metadata.",
  });
  const fetchBody = (url: string) =>
    http
      .execute(
        HttpClientRequest.get(url).pipe(
          HttpClientRequest.setHeader("Accept", "application/vnd.github+json"),
        ),
      )
      .pipe(
        Effect.flatMap((response) =>
          response.status === 404
            ? Effect.succeed(Option.none<string>())
            : HttpClientResponse.filterStatusOk(response).pipe(
                Effect.flatMap((ok) => ok.text),
                Effect.map(Option.some),
              ),
        ),
        Effect.timeout(Duration.seconds(15)),
        Effect.mapError(() => unreachable),
      );
  const cache = yield* Cache.make({
    capacity: 32,
    timeToLive: Duration.minutes(1),
    lookup: Effect.fn("NpmRelease.lookup")(function* (spec: string) {
      const track = spec === "latest" ? "stable" : spec === "nightly" ? "nightly" : null;
      if (track === null) {
        const body = yield* fetchBody(cliReleaseTagUrl(spec));
        if (Option.isNone(body)) return null;
        const release = yield* decodeRelease(body.value).pipe(Effect.mapError(() => invalid));
        return release.tag_name === `v${spec}` && release.draft !== true ? { version: spec } : null;
      }
      const body = yield* fetchBody(cliReleaseIndexPageUrl(1));
      if (Option.isNone(body)) return null;
      const releases = yield* decodeReleases(body.value).pipe(Effect.mapError(() => invalid));
      const version = newestCliReleaseVersion(releases, track);
      return version === undefined ? null : { version };
    }),
  });
  return NpmRelease.of({
    get: Effect.fn("NpmRelease.get")(function* (input) {
      const { spec } = yield* decodeInput(input).pipe(
        Effect.mapError(
          () => new ServerNpmReleaseError({ reason: "Invalid release version or track." }),
        ),
      );
      return yield* Cache.get(cache, spec);
    }),
  });
});

export const layer = Layer.effect(NpmRelease, make()).pipe(Layer.provide(FetchHttpClient.layer));
