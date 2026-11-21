import { describe, expect, it } from "vitest";
import { Background } from "./background.svelte";
import { empty } from "./chat";

describe("background sessions", () => {
  it("keeps the chat live, the canvas events, and marks the turn done", () => {
    const bg = new Background();
    bg.park("/s/a.jsonl", 7, "/work", { ...empty(), running: true }, { runError: null });
    expect(bg.sessionOf(7)).toBe("/s/a.jsonl");
    expect(bg.working).toEqual(["/s/a.jsonl"]);
    expect(bg.runningIn("/work")).toBe(true);

    bg.receive("/s/a.jsonl", { type: "message_update", assistantMessageEvent: { type: "text_delta", delta: "Hi" } });
    bg.receive("/s/a.jsonl", { type: "tool_execution_start", toolCallId: "1", toolName: "storyboard", args: {} });
    bg.receive("/s/a.jsonl", { type: "agent_settled" });

    const p = bg.parked["/s/a.jsonl"];
    expect(p.chat.running).toBe(false);
    expect(p.chat.items.some((i) => i.kind === "assistant" && i.text === "Hi")).toBe(true);
    expect(p.events.map((e) => e.type)).toEqual(["tool_execution_start", "agent_settled"]);
    expect(bg.working).toEqual([]);
    expect(bg.runningIn("/work")).toBe(false);

    expect(bg.take("/s/a.jsonl")).toBe(p);
    expect(bg.parked["/s/a.jsonl"]).toBeUndefined();
  });

  it("ignores events for a session no longer parked", () => {
    const bg = new Background();
    bg.receive("/s/gone.jsonl", { type: "agent_settled" });
    expect(bg.parked).toEqual({});
  });
});
