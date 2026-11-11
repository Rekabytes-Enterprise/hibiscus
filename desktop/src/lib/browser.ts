// Helpers for the browser pane, which shows the engine's browser tool.
// Pure functions, so they are unit-tested without a window.

/** Browser tool commands that act on one element, so the pane moves its cursor there. */
export const POINTING = new Set(["click", "dblclick", "fill", "type", "hover", "check", "uncheck", "select", "focus"]);

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

/** An element found by the pane's `hit` command, in page coordinates. */
export type Hit = { selector: string; text: string; url: string; x: number; y: number; width: number; height: number };

/** A comment pinned on an element; `image` is a cropped JPEG, base64. */
export type Note = { hit: Hit; text: string; image: string };

/** The most images one prompt takes. */
export const MAX_IMAGES = 4;

/** The message that hands the pane's comments to Hibi. Images are numbered in the order they're attached. */
export function commentMessage(notes: Note[]) {
  let images = 0;
  const lines = notes.map((n, i) => {
    const label = n.hit.text ? `"${n.hit.text}" ` : "";
    const image = n.image && images < MAX_IMAGES ? ` (image ${++images})` : "";
    return `${i + 1}. ${label}\`${n.hit.selector}\` on ${n.hit.url}${image}\n   ${n.text}`;
  });
  return `Comments on elements in the browser pane. Change the code for each one:\n\n${lines.join("\n\n")}`;
}
