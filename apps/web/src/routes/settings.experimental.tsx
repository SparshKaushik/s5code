import { createFileRoute } from "@tanstack/react-router";

import { ExperimentalSettingsPanel } from "../components/settings/ExperimentalSettings";

export const Route = createFileRoute("/settings/experimental")({
  component: ExperimentalSettingsPanel,
});
