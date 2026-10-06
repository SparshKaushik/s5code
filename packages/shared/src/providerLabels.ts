/** Older OpenCode catalogs omit vendor labels but retain provider-qualified slugs. */
export function resolveModelSubProvider(
  driver: string,
  model: { readonly slug: string; readonly subProvider?: string | undefined },
): string | undefined {
  const label = model.subProvider?.trim();
  if (label) return label;
  if (driver !== "opencode") return undefined;

  const separator = model.slug.indexOf("/");
  if (separator <= 0 || separator === model.slug.length - 1) return undefined;
  const providerId = model.slug.slice(0, separator);
  switch (providerId) {
    case "openai":
      return "OpenAI";
    case "opencode":
      return "OpenCode Zen";
    case "opencode-go":
      return "OpenCode Go";
    case "github-copilot":
      return "GitHub Copilot";
    default:
      return providerId
        .split(/[-_]+/)
        .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
        .join(" ");
  }
}
