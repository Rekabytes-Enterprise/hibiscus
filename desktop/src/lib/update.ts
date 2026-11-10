// What the update banner says while the app updates itself (#158). Pure, so it is unit-tested.

export type Progress =
  | { phase: "available" }
  | { phase: "downloading"; done: number; total: number | null }
  | { phase: "installing" }
  | { phase: "failed"; error: string };

/** The banner's status text, e.g. "Downloading… 42%". */
export function status(version: string, p: Progress): string {
  switch (p.phase) {
    case "available":
      return `Hibiscus ${version} is available`;
    case "downloading":
      return p.total ? `Downloading… ${Math.min(100, Math.floor((p.done * 100) / p.total))}%` : "Downloading…";
    case "installing":
      return "Installing…";
    case "failed":
      return `The update failed: ${p.error}`;
  }
}

/** Whether to offer `version`, given the version the user last dismissed. */
export const offered = (version: string, dismissed: string | null) => version !== dismissed;
