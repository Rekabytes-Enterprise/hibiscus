import { describe, expect, it } from "vitest";
import { apply, empty, entries, fromMessages, outputs, relative, summary, toolHunks, toolLabel, toolView, type Chat, type Tool } from "./chat";

const run = (events: any[], start: Chat = empty()) => events.reduce(apply, start);
const delta = (d: string) => ({ type: "message_update", assistantMessageEvent: { type: "text_delta", delta: d } });

describe("apply", () => {
  it("streams a reply and replaces it with the final message", () => {
    const chat = run([
      { type: "agent_start" },
      { type: "message_start", message: { role: "user", content: [{ type: "text", text: "hi" }] } },
      delta("Hel"),
      delta("lo"),
      { type: "message_end", message: { role: "assistant", stopReason: "stop", content: [{ type: "text", text: "Hello!" }] } },
      { type: "agent_settled" },
    ]);
    expect(chat.running).toBe(false);
    expect(chat.items).toEqual([
      { kind: "user", text: "hi", images: 0 },
      { kind: "assistant", text: "Hello!", streaming: false },
    ]);
  });

  it("tracks tools and closes streamed text before them", () => {
    const chat = run([
      delta("Let me look."),
      { type: "tool_execution_start", toolCallId: "t1", toolName: "bash", args: { command: "ls" } },
      {
        type: "tool_execution_end",
        toolCallId: "t1",
        toolName: "bash",
        isError: true,
        durationMs: 12,
        result: { content: [{ type: "text", text: "no such file" }] },
      },
    ]);
    expect(chat.items[0]).toEqual({ kind: "assistant", text: "Let me look.", streaming: false });
    expect(chat.items[1]).toMatchObject({ kind: "tool", running: false, error: true, output: "no such file", ms: 12 });
  });

  it("keeps partial text and shows the error when a run fails", () => {
    const chat = run([
      delta("partial"),
      { type: "message_end", message: { role: "assistant", content: [], stopReason: "error", errorMessage: "429 rate limit" } },
    ]);
    expect(chat.items).toEqual([
      { kind: "assistant", text: "partial", streaming: false },
      { kind: "notice", text: "429 rate limit", error: true },
    ]);
  });

  it("keeps queue, rejected prompts, waiting and engine stops", () => {
    let chat = run([
      { type: "agent_start" },
      { type: "queue_update", steering: ["a"], followUp: ["b"] },
      { type: "provider_status", provider: "rooxcode", waiting: true },
    ]);
    expect([chat.steering, chat.followUp, chat.waiting]).toEqual([["a"], ["b"], true]);
    chat = run([
      { type: "queue_rejected", message: "b", error: "Run failed before queued delivery" },
      { type: "engine_stopped", error: "boom" },
    ], chat);
    expect(chat.rejected).toEqual(["b"]);
    expect(chat.running).toBe(false);
    expect(chat.items.at(-1)).toMatchObject({ kind: "notice", error: true });
  });
});

describe("fromMessages", () => {
  it("rebuilds replies and tool results from a saved session", () => {
    const chat = fromMessages([
      { role: "user", content: [{ type: "text", text: "fix it" }, { type: "image", mimeType: "image/png", data: "" }] },
      {
        role: "assistant",
        stopReason: "toolUse",
        content: [{ type: "text", text: "Reading." }, { type: "toolCall", id: "c1", name: "read", arguments: { path: "a.rs" } }],
      },
      { role: "toolResult", toolCallId: "c1", toolName: "read", isError: false, content: [{ type: "text", text: "fn main() {}" }], details: {} },
    ]);
    expect(chat.items).toEqual([
      { kind: "user", text: "fix it", images: 1 },
      { kind: "assistant", text: "Reading.", streaming: false },
      { kind: "tool", id: "c1", name: "read", args: { path: "a.rs" }, running: false, error: false, output: "fn main() {}", diff: "", ms: null },
    ]);
  });
});

describe("toolLabel", () => {
  it("shows the first line of the main argument", () => {
    expect(toolLabel("bash", { command: "npm test\nmore" })).toBe("bash · npm test");
    expect(toolLabel("read", { path: "src/main.rs" })).toBe("read · src/main.rs");
    expect(toolLabel("goal", {})).toBe("goal");
  });
});

describe("outputs", () => {
  const tool = (name: string, path: string, isError = false) => [
    { type: "tool_execution_start", toolCallId: path + name, toolName: name, args: { path } },
    { type: "tool_execution_end", toolCallId: path + name, isError, result: { content: [] } },
  ];
  it("lists written and edited files once, newest first, skipping reads and failures", () => {
    const chat = run([
      ...tool("write", "a.ts"),
      ...tool("read", "b.ts"),
      ...tool("edit", "c.ts", true),
      ...tool("edit", "d.ts"),
      ...tool("edit", "a.ts"),
    ]);
    expect(outputs(chat)).toEqual(["a.ts", "d.ts"]);
  });
});

const tool = (name: string, args: any, extra: Partial<Tool> = {}): Tool => ({
  kind: "tool", id: name + JSON.stringify(args), name, args, running: false, error: false, output: "", diff: "", ms: null, ...extra,
});

describe("tool calls", () => {
  it("groups runs of two or more and leaves single calls alone", () => {
    const say = { kind: "assistant" as const, text: "hi", streaming: false };
    const shown = entries([tool("read", { path: "a" }), say, tool("bash", { command: "ls" }), tool("read", { path: "b" })]);
    expect(shown.map((e) => e.kind)).toEqual(["tool", "assistant", "tools"]);
  });

  it("summarises a group", () => {
    const tools = [tool("read", { path: "a" }), tool("read", { path: "b" }), tool("bash", { command: "ls" }), tool("edit", { path: "a" }), tool("web", {})];
    expect(summary(tools)).toBe("Read 2 files, made 1 edit, ran 1 command, used 1 tool");
  });

  it("reads like a sentence, with paths relative to the folder", () => {
    expect(toolView(tool("read", { path: "/w/src/main.rs" }), "/w")).toMatchObject({ verb: "Read", target: "src/main.rs", command: false });
    expect(toolView(tool("bash", { command: "npm test\nmore" }, { running: true }), "/w")).toMatchObject({ verb: "Running", target: "npm test", command: true });
    expect(relative("/other/x", "/w")).toBe("/other/x");
  });

  it("shortens session folder paths in commands", () => {
    const ran = (command: string) => toolView(tool("bash", { command }), "/w/app").target;
    expect(ran("rg foo /w/app/src /w/app")).toBe("rg foo src .");
    expect(ran("cd '/w/app' && ls /w/apple")).toBe("cd '.' && ls /w/apple");
  });

  it("merges back-to-back goal updates into the latest one", () => {
    const last = tool("goal", { n: 3 });
    const shown = entries([tool("goal", { n: 1 }), tool("goal", { n: 2 }), last]);
    expect(shown).toEqual([last]);
    const mixed = entries([tool("goal", { n: 1 }), tool("read", { path: "a" }), last]);
    expect(mixed[0].kind === "tools" && mixed[0].tools.length).toBe(3);
  });

  it("shows an edit's diff and a write as added lines", () => {
    const edit = tool("edit", { path: "a" }, { diff: "@@ -1,2 +1,2 @@\n a\n-b\n+c\n" });
    expect(toolHunks(edit)[0].lines.map((l) => l.kind)).toEqual(["ctx", "del", "add"]);
    const write = toolHunks(tool("write", { path: "a", content: "x\ny\n" }));
    expect(write[0].lines.map((l) => [l.kind, l.new, l.text])).toEqual([["add", 1, "x"], ["add", 2, "y"]]);
    expect(toolHunks(tool("read", { path: "a" }))).toEqual([]);
  });
});
