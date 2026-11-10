import { describe, expect, it } from "vitest";
import { calls, modifiers, toPage } from "./browser";

describe("calls", () => {
  it("finds the command and its target", () => {
    expect(calls("agent-browser click @e3")).toEqual([{ session: null, verb: "click", target: "@e3" }]);
    expect(calls("agent-browser snapshot -i")).toEqual([{ session: null, verb: "snapshot", target: null }]);
  });

  it("reads the session and skips other flags with values", () => {
    expect(calls("agent-browser --session dev --profile Default fill @e1 'a b'")).toEqual([
      { session: "dev", verb: "fill", target: "@e1" },
    ]);
    expect(calls("agent-browser --session=dev open http://localhost:5173")).toEqual([
      { session: "dev", verb: "open", target: "http://localhost:5173" },
    ]);
  });

  it("finds each call in a chained command", () => {
    expect(calls("cd app && agent-browser open localhost:3000; npx agent-browser click \"#go\" | cat").map((c) => [c.verb, c.target])).toEqual([
      ["open", "localhost:3000"],
      ["click", "#go"],
    ]);
  });

  it("ignores other commands", () => {
    expect(calls("npm test && echo agent-browser-ish")).toEqual([]);
    expect(calls("agent-browser")).toEqual([]);
  });
});

it("maps a pointer on the scaled frame to page coordinates", () => {
  expect(toPage(150, 60, { left: 100, top: 10, width: 640, height: 360 }, { width: 1280, height: 720 })).toEqual({ x: 100, y: 100 });
});

it("sets CDP modifier bits", () => {
  expect(modifiers({ altKey: false, ctrlKey: true, metaKey: false, shiftKey: true })).toBe(10);
});
