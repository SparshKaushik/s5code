import { useAtomValue } from "@effect/atom-react";
import {
  cliReleaseIndexPageUrl,
  cliReleaseTagUrl,
  newestCliReleaseVersion,
} from "@t3tools/shared/cliRelease";
import * as Effect from "effect/Effect";
import * as Schema from "effect/Schema";
import { FetchHttpClient, HttpClient, HttpClientResponse } from "effect/http";
import { AsyncResult, Atom } from "effect/reactivity";

export interface NpmRelease {
  readonly version: string;
  /** S5 Code releases carry no migration manifest, so a track switch that needs one stays blocked. */
  readonly databaseMigration: number | undefined;
}

const decodeGitHubReleases = Schema.decodeUnknownEffect(
  Schema.Array(Schema.Struct({ tag_name: Schema.String, draft: Schema.optional(Schema.Boolean) })),
);

/** `spec` is an exact version or a track (`latest`, `nightly`). S5 Code servers install from the
 * fork's GitHub Releases, not npm. Null when there is no such release. Refreshes only while
 * mounted; the published check below keeps its own versions alive. */
export const npmReleaseAtom = Atom.family((spec: string) =>
  Atom.make(
    Effect.gen(function* () {
      const track = spec === "latest" ? "stable" : spec === "nightly" ? "nightly" : null;
      const response = yield* HttpClient.get(
        track === null ? cliReleaseTagUrl(spec) : cliReleaseIndexPageUrl(1),
      );
      if (response.status === 404) return null;
      const body = yield* HttpClientResponse.filterStatusOk(response).pipe(
        Effect.flatMap((ok) => ok.json),
      );
      const version =
        track === null
          ? typeof body === "object" &&
            body !== null &&
            "tag_name" in body &&
            body.tag_name === `v${spec}`
            ? spec
            : undefined
          : newestCliReleaseVersion(yield* decodeGitHubReleases(body), track);
      return version === undefined
        ? null
        : ({ version, databaseMigration: undefined } satisfies NpmRelease);
    }).pipe(
      // Tracing headers would make the browser send a CORS preflight, which the release API rejects.
      Effect.provideService(HttpClient.TracerPropagationEnabled, false),
      Effect.provide(FetchHttpClient.layer),
    ),
  ).pipe(Atom.withRefresh("5 minutes"), Atom.withLabel(`npm-release:${spec}`)),
);

export const npmReleasePublishedAtom = Atom.family((version: string) =>
  Atom.make((get) =>
    get.result(npmReleaseAtom(version)).pipe(Effect.map((release) => release?.version === version)),
  ).pipe(Atom.keepAlive, Atom.withLabel(`npm-release-published:${version}`)),
);

const NO_VERSION_ATOM = Atom.make(AsyncResult.success(false)).pipe(
  Atom.withLabel("npm-release-published:none"),
);

/** True only once the release confirms the version, so callers can hide work that
    would download it until then. */
export function useNpmReleasePublished(version: string | null): boolean {
  const result = useAtomValue(version ? npmReleasePublishedAtom(version) : NO_VERSION_ATOM);
  return AsyncResult.getOrElse(result, () => false);
}
