// The folders the sidebar and the new-session screen list, in the order the
// user keeps them (#147). Using a folder doesn't move it; dragging does.
// Pure, so they are unit-tested.

/** After opening `path`: a new folder goes last, a known one stays where it is. */
export const opened = (recent: string[], path: string): string[] => (recent.includes(path) ? recent : [...recent, path]);

/** After adding `path` without opening it: the same as opening, it goes last. */
export const added = opened;

/** After removing `path` from the list. Its files and sessions are untouched. */
export const removed = (recent: string[], path: string): string[] => recent.filter((p) => p !== path);

/** After dragging `from` onto `to`: it takes `to`'s place, so dragging down lands below it and up above it. */
export function moved(recent: string[], from: string, to: string): string[] {
  const start = recent.indexOf(from);
  const end = recent.indexOf(to);
  if (start < 0 || end < 0 || start === end) return recent;
  const rest = recent.filter((p) => p !== from);
  return [...rest.slice(0, end), from, ...rest.slice(end)];
}
