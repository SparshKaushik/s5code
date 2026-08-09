import {
  createCheckpointMaintenanceEnvironmentAtoms,
  createRewindEnvironmentAtoms,
} from "@t3tools/client-runtime/state/rewind";

import { connectionAtomRuntime } from "../connection/runtime";

export const rewindEnvironment = createRewindEnvironmentAtoms(connectionAtomRuntime);
export const checkpointMaintenanceEnvironment =
  createCheckpointMaintenanceEnvironmentAtoms(connectionAtomRuntime);
