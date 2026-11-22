import { createTerminalEnvironmentAtoms } from "@t3tools/client-runtime/state/terminal";

import { connectionAtomRuntime } from "../connection/runtime";

export const terminalEnvironment: ReturnType<typeof createTerminalEnvironmentAtoms> =
  createTerminalEnvironmentAtoms(connectionAtomRuntime);
