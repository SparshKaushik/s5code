import { sign as signApplication, type SignOptions } from "@electron/osx-sign";

// macOS pins privacy grants for ad hoc builds to their exact hash, so every
// update silently drops them. A stable self-signed identity keeps them.
const selfSignIdentity = process.env.T3CODE_MACOS_SELF_SIGN_IDENTITY?.trim();

export default async function sign(options: SignOptions): Promise<void> {
  const selfSigned =
    options.identity === "-" && selfSignIdentity ? { identity: selfSignIdentity } : {};
  await signApplication({ ...options, ...selfSigned, batchCodesignCalls: true });
}
