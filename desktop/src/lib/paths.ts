/** "/Users/me/Desktop/Github" -> "Github". */
export const name = (path: string) => path.split("/").filter(Boolean).pop() ?? path;

/** Where a folder lives, "~/Desktop" style, for "Github · ~/Desktop" labels. */
export function where(path: string, home: string): string {
  const parent = "/" + path.split("/").filter(Boolean).slice(0, -1).join("/");
  const root = home.replace(/\/$/, "");
  return root && (parent === root || parent.startsWith(root + "/")) ? "~" + parent.slice(root.length) : parent;
}
