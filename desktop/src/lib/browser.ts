// Finds agent-browser commands in Hibi's shell calls, for the browser pane.
// Pure functions, so they are unit-tested without a window.

export type Call = {
  /** The `--session` name, or null for agent-browser's default session. */
  session: string | null;
  /** The command, e.g. `click`. */
  verb: string;
  /** The first argument after the command, e.g. `@e3`. */
  target: string | null;
};

/** Commands that act on one element, so the pane moves its cursor there first. */
export const POINTING = new Set(["click", "dblclick", "fill", "type", "hover", "check", "uncheck", "select", "focus"]);

// Global flags that take a value, so the value is not mistaken for the command.
const VALUED = new Set(["--session", "--profile", "--state", "--config", "--engine", "--provider", "-p", "--cdp", "--namespace", "--headers"]);

/** Splits on whitespace and shell operators; quotes are honoured, escapes are not. */
function words(command: string): string[] {
  const out: string[] = [];
  const re = /"([^"]*)"|'([^']*)'|(&&|\|\||[;|\n])|([^\s"';|&]+)/g;
  for (const m of command.matchAll(re)) out.push(m[1] ?? m[2] ?? (m[3] ? ";" : m[4]));
  return out;
}

/** Every agent-browser call in a shell command, in order. */
export function calls(command: string): Call[] {
  const w = words(command);
  const out: Call[] = [];
  for (let i = 0; i < w.length; i++) {
    if (!/(^|\/)agent-browser$/.test(w[i]) && !(w[i] === "npx" && w[i + 1] === "agent-browser")) continue;
    if (w[i] === "npx") i++;
    let session: string | null = null;
    let j = i + 1;
    for (; j < w.length && w[j].startsWith("-"); j++) {
      const [flag, value] = w[j].split("=", 2);
      const v = value ?? (VALUED.has(flag) ? w[++j] : undefined);
      if (flag === "--session" && v) session = v;
    }
    if (j >= w.length || w[j] === ";") continue;
    const target = w[j + 1] && w[j + 1] !== ";" && !w[j + 1].startsWith("-") ? w[j + 1] : null;
    out.push({ session, verb: w[j], target });
    i = j;
  }
  return out;
}

/** Turns a pointer position on the shown frame into page coordinates. */
export function toPage(x: number, y: number, rect: { left: number; top: number; width: number; height: number }, page: { width: number; height: number }) {
  return {
    x: Math.round(((x - rect.left) * page.width) / rect.width),
    y: Math.round(((y - rect.top) * page.height) / rect.height),
  };
}

/** CDP modifier bits: Alt 1, Ctrl 2, Meta 4, Shift 8. */
export const modifiers = (e: { altKey: boolean; ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }) =>
  (e.altKey ? 1 : 0) | (e.ctrlKey ? 2 : 0) | (e.metaKey ? 4 : 0) | (e.shiftKey ? 8 : 0);
