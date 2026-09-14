/**
 * Display names for provider driver kinds, shared by web and mobile.
 *
 * Both clients used to hardcode their own list, which is how pi shipped
 * looking right on web and as a raw instance id on mobile. Adding a driver
 * should mean editing one table.
 */

/**
 * Driver kinds whose display name is not just a title-cased slug. Anything
 * absent falls through to `formatProviderDriverName`'s title-casing, which is
 * the right answer for a fork's custom driver.
 */
const DRIVER_DISPLAY_NAMES: Readonly<Record<string, string>> = {
  codex: "Codex",
  claudeAgent: "Claude",
  // Threads persisted before the driver rename still carry the short form.
  claude: "Claude",
  cursor: "Cursor",
  grok: "Grok",
  opencode: "OpenCode",
  // pi brands itself lowercase; title-casing would render "Pi".
  pi: "pi",
};

/** Human-readable name for a provider driver kind. */
export function formatProviderDriverName(driver: string | null | undefined): string {
  if (!driver) return "This agent";
  const known = DRIVER_DISPLAY_NAMES[driver];
  if (known !== undefined) return known;

  // Title-case unknown driver kinds so a fork's driver still reads reasonably.
  const trimmed = driver.replace(/Agent$/i, "").trim();
  if (trimmed.length === 0) return driver;
  return trimmed.charAt(0).toUpperCase() + trimmed.slice(1);
}

/**
 * Label for a configured provider *instance*.
 *
 * A user-set display name wins, then the driver's name. The instance id is the
 * last resort: it is unique but not descriptive, so it only shows for a driver
 * this build does not know.
 */
export function formatProviderInstanceLabel(instance: {
  readonly displayName?: string | undefined;
  readonly driver: string;
  readonly instanceId: string;
}): string {
  const displayName = instance.displayName?.trim();
  if (displayName) return displayName;
  const driverName = formatProviderDriverName(instance.driver);
  return driverName === "This agent" ? instance.instanceId : driverName;
}
