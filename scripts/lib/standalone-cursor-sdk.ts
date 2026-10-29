/**
 * The flattened SDK still discovers native helpers beside the entry script.
 * A standalone binary extracts those helpers to a cache before importing it.
 * Apply this only during Bun compilation, leaving disk-backed SDKs untouched.
 */
export function patchStandaloneCursorSdk(source: string): string {
  // The SDK's helper resolver, matched by shape because the minifier renames it each release.
  // Fail the build when the shape changes rather than silently losing native tool support.
  const resolver =
    /function \w+\(\$\)\{(?=let\{relativePath:J,accept:Z\}=\$,X=`\$\{process\.platform\}-\$\{process\.arch\}`)/g;
  if (source.match(resolver)?.length !== 1) {
    throw new Error("Cursor SDK native-helper resolver changed; update standalone packaging.");
  }
  return source.replace(
    resolver,
    (head) =>
      `${head}const directory=process.env.T3CODE_CURSOR_SDK_PLATFORM_DIR;if(directory){const candidate=directory+"/"+$.relativePath;if($.accept(candidate))return candidate;}`,
  );
}
