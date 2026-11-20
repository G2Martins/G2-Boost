import { boot } from "@/lib/boot";
import { navigate } from "@/lib/nav";

/** What another screen asked the disk page to show on arrival (taken once). */
export type DiskIntent = {
  tab?: "treemap" | "largest" | "dupes" | "essentials";
  /** Filter for the largest-files tab (`>500mb >6m`). */
  query?: string;
  /** No scan in memory yet: start one of this drive (`C:`) right away. */
  scan?: string;
};

let pending: DiskIntent | null = null;

/** Open the disk page on a tab, already filtered (from the cleanup page's next step). */
export function openDisk(intent: DiskIntent) {
  pending = intent;
  navigate("disk");
}

export function takeDiskIntent(): DiskIntent {
  const i = pending ?? {};
  pending = null;
  return i;
}

/** Apps › leftovers, straight on the every-drive "no owner" sweep. */
export function openUnowned() {
  boot.startTab = "unowned";
  navigate("apps");
}
