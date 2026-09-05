import { createCheckpointMaintenanceEnvironmentAtoms } from "@t3tools/client-runtime/state/checkpointMaintenance";

import { connectionAtomRuntime } from "../connection/runtime";

export const checkpointMaintenanceEnvironment =
  createCheckpointMaintenanceEnvironmentAtoms(connectionAtomRuntime);
