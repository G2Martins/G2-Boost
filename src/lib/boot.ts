/** Dev aid filled from `app_info` at boot: `G2BOOST_START_ROUTE` / `G2BOOST_START_TAB` env vars. */
export const boot: { startTab?: string; autorun?: boolean; scanPath?: string } = {};

/**
 * The tab a page was asked to open on (`goTo`, a banner, the boot env var), read once: the next
 * visit opens on the page's default. Pages call it in a `useState` initializer, once per mount.
 */
export function takeStartTab(): string | undefined {
  const tab = boot.startTab;
  boot.startTab = undefined;
  return tab;
}
