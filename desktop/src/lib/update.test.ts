import { expect, it } from "vitest";
import { offered, status } from "./update";

it("describes each step of an update", () => {
  expect(status("0.3.34", { phase: "available" })).toBe("Hibiscus 0.3.34 is available");
  expect(status("0.3.34", { phase: "downloading", done: 421, total: 1000 })).toBe("Downloading… 42%");
  expect(status("0.3.34", { phase: "downloading", done: 1200, total: 1000 })).toBe("Downloading… 100%");
  expect(status("0.3.34", { phase: "downloading", done: 10, total: null })).toBe("Downloading…");
  expect(status("0.3.34", { phase: "installing" })).toBe("Installing…");
  expect(status("0.3.34", { phase: "failed", error: "bad signature" })).toBe("The update failed: bad signature");
});

it("hides only the dismissed version", () => {
  expect(offered("0.3.34", null)).toBe(true);
  expect(offered("0.3.34", "0.3.34")).toBe(false);
  expect(offered("0.3.35", "0.3.34")).toBe(true);
});
