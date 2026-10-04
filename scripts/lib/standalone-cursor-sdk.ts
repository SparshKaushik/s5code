/**
 * The flattened SDK still discovers native helpers beside the entry script.
 * A standalone binary extracts those helpers to a cache before importing it.
 * Apply this only during Bun compilation, leaving disk-backed SDKs untouched.
 */
export function patchStandaloneCursorSdk(source: string): string {
  // This is the helper resolver in the pinned @cursor/sdk 1.0.31 bundle. Fail
  // the build on SDK upgrades rather than silently losing native tool support.
  const signature = "function RZ1($){let{relativePath:Z,accept:X}=$,";
  if (source.split(signature).length !== 2) {
    throw new Error("Cursor SDK native-helper resolver changed; update standalone packaging.");
  }
  return source.replace(
    signature,
    'function RZ1($){const directory=process.env.T3CODE_CURSOR_SDK_PLATFORM_DIR;if(directory){const candidate=directory+"/"+$.relativePath;if($.accept(candidate))return candidate;}let{relativePath:Z,accept:X}=$,',
  );
}
