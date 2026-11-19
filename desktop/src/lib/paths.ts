// Windows paths use `\`; the engine hands over native paths on every platform.
const parts = (path: string) => path.split(/[\\/]/).filter(Boolean);

/** "/Users/me/Desktop/Github" -> "Github", and "C:\Users\me\Github" -> "Github". */
export const name = (path: string) => parts(path).pop() ?? path;

/** Where a folder lives, "~/Desktop" style, for "Github · ~/Desktop" labels. */
export function where(path: string, home: string): string {
  const sep = path.includes("\\") ? "\\" : "/";
  const up = parts(path).slice(0, -1);
  const parent = sep === "/" ? "/" + up.join("/") : up.join("\\");
  const root = home.replace(/[\\/]$/, "");
  return root && (parent === root || parent.startsWith(root + sep)) ? "~" + parent.slice(root.length) : parent;
}

/** The WSL distro a folder lives in: `\\wsl.localhost\Ubuntu\home\me` is in "Ubuntu" (#206). */
export const wslDistro = (path: string) =>
  /^\\\\(?:\?\\UNC\\)?(?:wsl\.localhost|wsl\$)\\([^\\]+)/i.exec(path.replace(/\//g, "\\"))?.[1];
