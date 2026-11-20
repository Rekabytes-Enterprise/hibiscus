// What the sidebar's update row says while the app updates itself (#158). Pure, so it is unit-tested.

export type Progress =
  | { phase: "available" }
  | { phase: "downloading"; done: number; total: number | null }
  | { phase: "installing" }
  | { phase: "failed"; error: string };

/** The update row's text, e.g. "Downloading… 42%". */
export function status(version: string, p: Progress): string {
  switch (p.phase) {
    case "available":
      return `Relaunch to update to ${version}`;
    case "downloading":
      return p.total ? `Downloading… ${Math.min(100, Math.floor((p.done * 100) / p.total))}%` : "Downloading…";
    case "installing":
      return "Installing…";
    case "failed":
      return "Update failed. Try again";
  }
}

/** Whether to offer `version`, given the version the user last dismissed. */
export const offered = (version: string, dismissed: string | null) => version !== dismissed;
