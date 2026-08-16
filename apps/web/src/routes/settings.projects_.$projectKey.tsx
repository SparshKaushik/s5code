import { createFileRoute } from "@tanstack/react-router";

import { ProjectSettingsPanel } from "../components/settings/ProjectSettingsPanel";

function SettingsProjectDetailRoute() {
  const { projectKey } = Route.useParams();
  return <ProjectSettingsPanel projectKey={projectKey} />;
}

export const Route = createFileRoute("/settings/projects_/$projectKey")({
  component: SettingsProjectDetailRoute,
});
