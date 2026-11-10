// Turns engine events and saved messages into what the chat view shows.
// Pure functions, so they are unit-tested without a window.
import { parseDiff, type Hunk } from "./diff";
import type { Record } from "./engine";

export type Item =
  | { kind: "user"; text: string; images: number }
  | { kind: "assistant"; text: string; streaming: boolean }
  | {
      kind: "tool";
      id: string;
      name: string;
      args: any;
      running: boolean;
      error: boolean;
      output: string;
      /** Unified diff of an edit, from the engine's result details. */
      diff: string;
      ms: number | null;
    }
  | { kind: "notice"; text: string; error: boolean };

export type Chat = {
  items: Item[];
  running: boolean;
  steering: string[];
  followUp: string[];
  /** Shown while the Rooxcode Gateway queues the request. */
  waiting: boolean;
  /** Prompts the engine could not deliver; offered for resending. */
  rejected: string[];
};

export const empty = (): Chat => ({
  items: [],
  running: false,
  steering: [],
  followUp: [],
  waiting: false,
  rejected: [],
});

type Block = { type: string; text?: string; id?: string; name?: string; arguments?: any };

const text = (content: Block[] = []) =>
  content
    .filter((b) => b.type === "text")
    .map((b) => b.text ?? "")
    .join("\n");

/** Short description of a tool call, e.g. `bash · npm test`. */
export function toolLabel(name: string, args: any): string {
  const detail = args?.command ?? args?.path ?? args?.pattern ?? args?.query ?? "";
  const line = String(detail).split("\n")[0];
  return line ? `${name} · ${line.length > 80 ? line.slice(0, 79) + "…" : line}` : name;
}

function finishStreaming(items: Item[]): Item[] {
  return items.map((i) => (i.kind === "assistant" && i.streaming ? { ...i, streaming: false } : i));
}

/** Applies one engine event. Returns a new state. */
export function apply(chat: Chat, event: Record): Chat {
  const items = chat.items;
  const last = items[items.length - 1];
  switch (event.type) {
    case "agent_start":
      return { ...chat, running: true };
    case "agent_settled":
      return { ...chat, running: false, waiting: false, items: finishStreaming(items) };
    case "message_start": {
      const m = event.message;
      if (m?.role !== "user") return chat;
      const images = (m.content ?? []).filter((b: Block) => b.type === "image").length;
      return { ...chat, items: [...items, { kind: "user", text: text(m.content), images }] };
    }
    case "message_update": {
      const delta = event.assistantMessageEvent?.delta;
      if (event.assistantMessageEvent?.type !== "text_delta" || !delta) return chat;
      if (last?.kind === "assistant" && last.streaming) {
        return { ...chat, waiting: false, items: [...items.slice(0, -1), { ...last, text: last.text + delta }] };
      }
      return { ...chat, waiting: false, items: [...items, { kind: "assistant", text: delta, streaming: true }] };
    }
    case "message_end": {
      const m = event.message;
      if (m?.role !== "assistant") return chat;
      const rest = last?.kind === "assistant" && last.streaming ? items.slice(0, -1) : items;
      if (m.stopReason === "error") {
        const partial = last?.kind === "assistant" && last.streaming && last.text
          ? [{ ...last, streaming: false }]
          : [];
        return {
          ...chat,
          items: [...rest, ...partial, { kind: "notice", text: m.errorMessage ?? "The run failed.", error: true }],
        };
      }
      // The final message replaces streamed text (retries may have restarted it).
      const final = text(m.content);
      return final
        ? { ...chat, items: [...rest, { kind: "assistant", text: final, streaming: false }] }
        : { ...chat, items: rest };
    }
    case "tool_execution_start":
      return {
        ...chat,
        items: [
          ...finishStreaming(items),
          {
            kind: "tool",
            id: event.toolCallId,
            name: event.toolName,
            args: event.args,
            running: true,
            error: false,
            output: "",
            diff: "",
            ms: null,
          },
        ],
      };
    case "tool_execution_end":
      return {
        ...chat,
        items: items.map((i) =>
          i.kind === "tool" && i.id === event.toolCallId
            ? {
                ...i,
                running: false,
                error: !!event.isError,
                output: text(event.result?.content),
                diff: event.result?.details?.diff ?? "",
                ms: event.durationMs ?? null,
              }
            : i,
        ),
      };
    case "queue_update":
      return { ...chat, steering: event.steering ?? [], followUp: event.followUp ?? [] };
    case "queue_rejected":
      return {
        ...chat,
        rejected: [...chat.rejected, event.message],
        items: [...items, { kind: "notice", text: event.error, error: true }],
      };
    case "provider_status":
      return { ...chat, waiting: !!event.waiting };
    case "auto_retry_start":
      return { ...chat, items: [...items, { kind: "notice", text: "Retrying the provider request…", error: false }] };
    case "compaction_start":
      return { ...chat, items: [...items, { kind: "notice", text: "Compacting the conversation…", error: false }] };
    case "compaction_end":
      return event.errorMessage
        ? { ...chat, items: [...items, { kind: "notice", text: event.errorMessage, error: true }] }
        : chat;
    case "provider_error":
    case "extension_error":
      return { ...chat, items: [...items, { kind: "notice", text: event.error, error: true }] };
    case "extension_command_output":
      return { ...chat, items: [...items, { kind: "notice", text: event.text, error: false }] };
    case "engine_stopped":
      return {
        ...chat,
        running: false,
        items: [...finishStreaming(items), { kind: "notice", text: `The engine stopped: ${event.error}`, error: true }],
      };
    default:
      return chat;
  }
}

/** Rebuilds the chat from a saved conversation (`get_messages`). */
export function fromMessages(messages: any[]): Chat {
  const items: Item[] = [];
  for (const m of messages) {
    if (m.role === "user") {
      const images = (m.content ?? []).filter((b: Block) => b.type === "image").length;
      items.push({ kind: "user", text: text(m.content), images });
    } else if (m.role === "assistant") {
      const reply = text(m.content);
      if (reply) items.push({ kind: "assistant", text: reply, streaming: false });
      for (const b of (m.content ?? []) as Block[]) {
        if (b.type === "toolCall") {
          items.push({
            kind: "tool",
            id: b.id ?? "",
            name: b.name ?? "tool",
            args: b.arguments,
            running: false,
            error: false,
            output: "",
            diff: "",
            ms: null,
          });
        }
      }
    } else if (m.role === "toolResult") {
      const tool = items.find((i) => i.kind === "tool" && i.id === m.toolCallId);
      if (tool?.kind === "tool") {
        tool.output = text(m.content);
        tool.error = !!m.isError;
        tool.diff = m.details?.diff ?? "";
      }
    }
  }
  return { ...empty(), items };
}

/** Files this session wrote or edited, newest first, for the session panel. */
export function outputs(chat: Chat): string[] {
  const paths: string[] = [];
  for (const item of chat.items) {
    if (item.kind !== "tool" || item.error || !["write", "edit"].includes(item.name)) continue;
    const path = item.args?.path ?? item.args?.filePath;
    if (typeof path === "string" && path) paths.unshift(path);
  }
  return [...new Set(paths)];
}

export type Tool = Extract<Item, { kind: "tool" }>;
/** What the chat shows: items, with runs of two or more tool calls folded into one group. */
export type Entry = Item | { kind: "tools"; tools: Tool[] };

export function entries(items: Item[]): Entry[] {
  const out: Entry[] = [];
  let run: Tool[] = [];
  const flush = () => {
    if (run.length > 1) out.push({ kind: "tools", tools: run });
    else out.push(...run);
    run = [];
  };
  for (const item of items) {
    // Back-to-back goal updates show as one row: the latest checklist.
    if (item.kind === "tool" && item.name === "goal" && run.at(-1)?.name === "goal") run[run.length - 1] = item;
    else if (item.kind === "tool") run.push(item);
    else {
      flush();
      out.push(item);
    }
  }
  flush();
  return out;
}

// [while running, done, icon]
const VERBS: { [tool: string]: [string, string, string] } = {
  read: ["Reading", "Read", "file-text"],
  write: ["Writing", "Wrote", "edit"],
  edit: ["Editing", "Edited", "edit"],
  bash: ["Running", "Ran", "terminal"],
  goal: ["Updating", "Updated", "check"],
};

/** `path` relative to the session folder when it is inside it. */
export function relative(path: string, folder: string | null): string {
  const root = folder?.replace(/\/$/, "");
  return root && path.startsWith(root + "/") ? path.slice(root.length + 1) : path;
}

/** `command` with paths inside the session folder written relative to it, and the folder itself as `.`. */
function shorten(command: string, folder: string | null): string {
  const root = folder?.replace(/\/$/, "");
  if (!root) return command;
  const at = root.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return command.replace(new RegExp(at + "/", "g"), "").replace(new RegExp(at + "(?=$|[\\s'\"])", "g"), ".");
}

/** One tool call as a sentence: "Read src/main.rs", "Ran npm test". */
export function toolView(tool: Tool, folder: string | null) {
  const [running, done, icon] = VERBS[tool.name] ?? ["Using", "Used", "cpu"];
  const a = tool.args ?? {};
  const command = tool.name === "bash";
  const raw = command ? shorten(String(a.command ?? "").split("\n")[0], folder) : tool.name === "goal" ? "goals" : a.path ? relative(String(a.path), folder) : toolLabel(tool.name, a);
  const target = raw.length > 120 ? raw.slice(0, 119) + "…" : raw;
  return { verb: tool.running ? running : done, target, icon, command };
}

/** "Read 3 files, ran 2 commands" for a group of tool calls. */
export function summary(tools: Tool[]): string {
  const count = (names: string[]) => tools.filter((t) => names.includes(t.name)).length;
  const n = (k: number, one: string, many = one + "s") => `${k} ${k === 1 ? one : many}`;
  const parts: string[] = [];
  const reads = count(["read"]);
  const writes = count(["write"]);
  const edits = count(["edit"]);
  const runs = count(["bash"]);
  const goals = count(["goal"]);
  const other = tools.length - reads - writes - edits - runs - goals;
  if (reads) parts.push(`read ${n(reads, "file")}`);
  if (edits) parts.push(`made ${n(edits, "edit")}`);
  if (writes) parts.push(`wrote ${n(writes, "file")}`);
  if (runs) parts.push(`ran ${n(runs, "command")}`);
  if (goals) parts.push("updated goals");
  if (other) parts.push(`used ${n(other, "tool")}`);
  const text = parts.join(", ");
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/** Lines shown for a write: the new file, as all-added lines. */
const WRITE_PREVIEW = 200;

/** The change a write or edit made, as diff hunks; empty for other tools. */
export function toolHunks(tool: Tool): Hunk[] {
  let body = tool.diff;
  if (!body && tool.name === "write" && typeof tool.args?.content === "string") {
    const lines = tool.args.content.replace(/\n$/, "").split("\n").slice(0, WRITE_PREVIEW);
    body = `@@ -0,0 +1,${lines.length} @@\n` + lines.map((l: string) => "+" + l).join("\n");
  }
  if (!body) return [];
  return parseDiff("diff --git a/f b/f\n" + body)[0]?.hunks ?? [];
}

/**
 * Conversations opened during this run, by session file, so going back to one
 * shows it at once (#159). Holds the most recent `limit`; nothing is preloaded.
 */
export class ChatCache {
  #chats = new Map<string, Chat>();
  constructor(private limit = 20) {}

  get(path: string): Chat | undefined {
    return this.#chats.get(path);
  }

  keep(path: string, chat: Chat) {
    this.#chats.delete(path);
    this.#chats.set(path, chat);
    if (this.#chats.size > this.limit) this.#chats.delete(this.#chats.keys().next().value!);
  }
}

// What the session file holds: notices and tool timings only exist while the app watches a run.
const saved = (chat: Chat) =>
  JSON.stringify(
    chat.items.flatMap((i) =>
      i.kind === "tool" ? [[i.id, i.output, i.error]] : i.kind === "notice" ? [] : [[i.kind, i.text]],
    ),
  );

/** `cached` if the saved conversation is unchanged, so its rows stay as they are; else `fresh`. */
export const newer = (cached: Chat, fresh: Chat): Chat => (saved(cached) === saved(fresh) ? cached : fresh);
