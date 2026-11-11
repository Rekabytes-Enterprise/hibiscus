import { expect, it } from "vitest";
import { commentMessage, modifiers, toPage } from "./browser";

it("maps a pointer on the scaled frame to page coordinates", () => {
  expect(toPage(150, 60, { left: 100, top: 10, width: 640, height: 360 }, { width: 1280, height: 720 })).toEqual({ x: 100, y: 100 });
});

it("sets CDP modifier bits", () => {
  expect(modifiers({ altKey: false, ctrlKey: true, metaKey: false, shiftKey: true })).toBe(10);
});

it("lists each comment with its element, page and image", () => {
  const hit = { selector: "#save", text: "Save", url: "http://localhost:5173/", x: 0, y: 0, width: 10, height: 10 };
  const notes = [1, 2, 3, 4, 5].map((n) => ({ hit, text: `note ${n}`, image: "AAAA" }));
  notes[1] = { hit: { ...hit, text: "" }, text: "make it red", image: "" };
  const message = commentMessage(notes);
  expect(message).toContain('1. "Save" `#save` on http://localhost:5173/ (image 1)\n   note 1');
  expect(message).toContain("2. `#save` on http://localhost:5173/\n   make it red");
  expect(message).toContain("5. \"Save\" `#save` on http://localhost:5173/ (image 4)");
  expect(message).not.toContain("(image 5)");
});
