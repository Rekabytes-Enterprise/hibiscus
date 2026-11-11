import { describe, expect, it } from "vitest";
import { apply, ChatCache, empty, entries, newer, fromMessages, outputs, relative, summary, toolHunks, toolLabel, toolView, type Chat, type Tool } from "./chat";

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
      { kind: "tool", id: "c1", name: "read", args: { path: "a.rs" }, running: false, error: false, output: "fn main() {}", diff: "", image: "", trimmed: false, pendingImage: false, ms: null },
    ]);
  });

  it("keeps a screenshot a tool returned", () => {
    const chat = fromMessages([
      { role: "assistant", content: [{ type: "toolCall", id: "c1", name: "browser", arguments: { args: ["screenshot"] } }] },
      { role: "toolResult", toolCallId: "c1", content: [{ type: "text", text: "saved" }, { type: "image", mimeType: "image/jpeg", data: "AAAA" }] },
    ]);
    expect(chat.items[0]).toMatchObject({ output: "saved", image: "data:image/jpeg;base64,AAAA" });
  });
});

describe("pages of a long session", () => {
  it("marks rows the page trimmed or left an image out of", () => {
    const chat = fromMessages([
      { role: "assistant", content: [{ type: "toolCall", id: "c1", name: "browser", arguments: { args: ["screenshot"] } }] },
      { role: "toolResult", toolCallId: "c1", content: [{ type: "text", text: "saved" }], trimmed: true, omittedImages: 1 },
    ]);
    expect(chat.items[0]).toMatchObject({ output: "saved", image: "", trimmed: true, pendingImage: true });
  });

  it("lists edits from pages not loaded yet after the loaded ones", () => {
    const chat = fromMessages([
      { role: "assistant", content: [{ type: "toolCall", id: "c1", name: "edit", arguments: { path: "new.rs" } }] },
      { role: "toolResult", toolCallId: "c1", content: [] },
    ]);
    expect(outputs({ ...chat, edited: ["new.rs", "old.rs"] })).toEqual(["new.rs", "old.rs"]);
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
  kind: "tool", id: name + JSON.stringify(args), name, args, running: false, error: false, output: "", diff: "", image: "", trimmed: false, pendingImage: false, ms: null, ...extra,
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
    expect(toolView(tool("browser", { args: ["click", "@e3"] }), "/w")).toMatchObject({ verb: "Browsed", target: "click @e3", icon: "globe" });
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

describe("ChatCache", () => {
  const conversation = (reply: string) =>
    fromMessages([
      { role: "user", content: [{ type: "text", text: "hi" }] },
      { role: "assistant", content: [{ type: "text", text: reply }] },
    ]);

  it("returns a conversation opened before", () => {
    const cache = new ChatCache();
    const chat = conversation("hello");
    cache.keep("/sessions/a.jsonl", chat);
    expect(cache.get("/sessions/a.jsonl")).toBe(chat);
    expect(cache.get("/sessions/b.jsonl")).toBeUndefined();
  });

  it("keeps the cached rows while the session file is unchanged", () => {
    const cached = run([{ type: "auto_retry_start" }], conversation("hello"));
    expect(newer(cached, conversation("hello"))).toBe(cached);
  });

  it("takes the fresh conversation once the session file changed", () => {
    const fresh = fromMessages([
      { role: "user", content: [{ type: "text", text: "hi" }] },
      { role: "assistant", content: [{ type: "text", text: "hello" }] },
      { role: "user", content: [{ type: "text", text: "from the TUI" }] },
    ]);
    expect(newer(conversation("hello"), fresh)).toBe(fresh);
  });

  it("keeps the cached rows when the engine sends only the latest page of them", () => {
    const cached = fromMessages([
      { role: "user", content: [{ type: "text", text: "older" }] },
      { role: "user", content: [{ type: "text", text: "hi" }] },
      { role: "assistant", content: [{ type: "text", text: "hello" }] },
    ]);
    const page = { ...conversation("hello"), start: 1 };
    expect(newer(cached, page)).toBe(cached);
    // The whole conversation is compared in full: here it lost a message, so it wins.
    const whole = { ...page, start: 0 };
    expect(newer(cached, whole)).toBe(whole);
    expect(newer(cached, { ...conversation("changed"), start: 1 }).items[1]).toMatchObject({ text: "changed" });
  });

  it("keeps sessions from different folders apart", () => {
    const cache = new ChatCache();
    const a = conversation("in a");
    const b = conversation("in b");
    cache.keep("/home/me/a/.sessions/1.jsonl", a);
    cache.keep("/home/me/b/.sessions/1.jsonl", b);
    expect(cache.get("/home/me/a/.sessions/1.jsonl")).toBe(a);
    expect(cache.get("/home/me/b/.sessions/1.jsonl")).toBe(b);
  });

  it("drops the least recently kept conversation past its limit", () => {
    const cache = new ChatCache(2);
    cache.keep("1", empty());
    cache.keep("2", empty());
    cache.keep("1", empty());
    cache.keep("3", empty());
    expect(cache.get("2")).toBeUndefined();
    expect(cache.get("1")).toBeDefined();
    expect(cache.get("3")).toBeDefined();
  });
});
