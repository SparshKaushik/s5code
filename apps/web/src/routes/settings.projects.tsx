import { createFileRoute } from "@tanstack/react-router";

import { ProjectSettingsPanel } from "../components/settings/ProjectSettingsPanel";

function SettingsProjectsRoute() {
  return <ProjectSettingsPanel projectKey={null} />;
}

export const Route = createFileRoute("/settings/projects")({
  component: SettingsProjectsRoute,
});
