// The folders the app remembers, newest first, as the sidebar and the
// new-session screen show them (#147). Pure, so they are unit-tested.

/** How many folders the app remembers. */
export const LIMIT = 8;

/** After opening `path`: it moves to the front. */
export const opened = (recent: string[], path: string): string[] =>
  [path, ...recent.filter((p) => p !== path)].slice(0, LIMIT);

/**
 * After adding `path` without opening it. It goes second, behind the folder
 * that is open, because the app reopens the first folder on its next start.
 * A folder already in the list stays where it is.
 */
export function added(recent: string[], path: string): string[] {
  if (recent.includes(path)) return recent;
  return [...recent.slice(0, 1), path, ...recent.slice(1)].slice(0, LIMIT);
}

/** After removing `path` from the list. Its files and sessions are untouched. */
export const removed = (recent: string[], path: string): string[] => recent.filter((p) => p !== path);
