import { expect, it } from "vitest";
import { modifiers, toPage } from "./browser";

it("maps a pointer on the scaled frame to page coordinates", () => {
  expect(toPage(150, 60, { left: 100, top: 10, width: 640, height: 360 }, { width: 1280, height: 720 })).toEqual({ x: 100, y: 100 });
});

it("sets CDP modifier bits", () => {
  expect(modifiers({ altKey: false, ctrlKey: true, metaKey: false, shiftKey: true })).toBe(10);
});
