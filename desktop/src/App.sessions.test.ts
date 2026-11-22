// @vitest-environment jsdom
import { mount, unmount } from "svelte";
import { expect, it, vi } from "vitest";
import App from "./App.svelte";

const tauri = vi.hoisted(() => ({
  invoke: vi.fn(),
  events: new Map<string, (event: { payload: unknown }) => void>(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke, convertFileSrc: (path: string) => path }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, handler: (event: { payload: unknown }) => void) => {
    tauri.events.set(name, handler);
    return Promise.resolve(() => {});
  },
}));
vi.mock("@tauri-apps/api/path", () => ({ homeDir: async () => "/work" }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ ask: async () => true, open: async () => null }));
vi.mock("./lib/About.svelte", () => ({ default: () => {} }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: async () => null }));

it("replaces the open session before deleting it and can then resume another", async () => {
  localStorage.setItem("hibiscus.ownProvider", "1");
  const first = "/sessions/first.jsonl";
  const next = "/sessions/next.jsonl";
  let sessions = [
    { path: first, title: "First session", modified: 2 },
    { path: next, title: "Next session", modified: 1 },
  ];
  let current: string | null = null;
  let worker = false;
  let sequence = 0;
  tauri.invoke.mockImplementation(async (name: string, args?: { session?: string; path?: string; command?: { type: string; sessionPath?: string } }) => {
    if (name === "open_project") {
      worker = true;
      current = args?.session ?? null;
      return false;
    }
    if (name === "sessions") return [...sessions];
    if (name === "delete_session") {
      if (current === args?.path) throw new Error("The engine still owns the deleted session");
      sessions = sessions.filter((s) => s.path !== args?.path);
      // Simulate a worker disappearing while the UI still shows the folder.
      worker = false;
      return;
    }
    if (name === "send") {
      if (!worker) throw "No project is open.";
      const command = args?.command;
      if (command?.type === "switch_session") current = command.sessionPath ?? null;
      if (command?.type === "new_session") current = null;
      const id = `desktop-${++sequence}`;
      tauri.events.get("engine")?.({ payload: {
        type: "response", id, success: true,
        data: { sessionFile: current, hasAccess: true, levels: [], messages: [], models: [] },
      } });
      return id;
    }
    if (name === "wsl_distros" || name === "canvas_styles") return [];
    return null;
  });
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(App, { target });
  const button = (title: string) => target.querySelector<HTMLButtonElement>(`button[title="${title}"]`);
  try {
    await vi.waitFor(() => expect(button("First session")?.disabled).toBe(false));
    button("First session")?.click();
    const menu = () => button("First session")?.closest("li.active")?.querySelector<HTMLButtonElement>(
      '[aria-label="Session actions"]',
    );
    await vi.waitFor(() => expect(menu()?.disabled).toBe(false));
    menu()?.click();
    const deleteAction = () => Array.from(target.querySelectorAll<HTMLButtonElement>('button[role="menuitem"]'))
      .find((item) => item.textContent?.trim() === "Delete");
    await vi.waitFor(() => expect(deleteAction()).toBeDefined());
    deleteAction()?.click();
    await vi.waitFor(() => {
      expect(button("First session")).toBeNull();
      expect(button("Next session")?.disabled).toBe(false);
    });
    button("Next session")?.click();
    await vi.waitFor(() => expect(current).toBe(next));
    await vi.waitFor(() => expect(target.querySelector("li.active")?.textContent).toContain("Next session"));
    expect(worker).toBe(true);
    expect(tauri.invoke).toHaveBeenCalledWith("delete_session", { path: first });
    expect(target.textContent).not.toContain("No project is open.");
  } finally {
    await unmount(app);
    target.remove();
    localStorage.clear();
  }
});

it("runs /compact for the current session and refuses to queue it in an active turn", async () => {
  localStorage.setItem("hibiscus.ownProvider", "1");
  const originalScrollTo = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "scrollTo");
  Object.defineProperty(HTMLElement.prototype, "scrollTo", { configurable: true, value: vi.fn() });
  let sequence = 0;
  const commands: { type: string; customInstructions?: string }[] = [];
  let compactId: string | undefined;
  tauri.invoke.mockImplementation(async (name: string, args?: { command?: { type: string; customInstructions?: string } }) => {
    if (name === "open_project") return false;
    if (name === "sessions" || name === "wsl_distros" || name === "canvas_styles") return [];
    if (name === "send" && args?.command) {
      commands.push(args.command);
      const id = `desktop-compact-test-${++sequence}`;
      if (args.command.type === "compact") {
        compactId = id;
      } else {
        tauri.events.get("engine")?.({ payload: { type: "response", id, success: true,
          data: { hasAccess: true, messages: [], models: [], levels: [] },
        } });
      }
      return id;
    }
    return null;
  });
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(App, { target });
  async function draft(text: string) {
    const input = target.querySelector("textarea");
    if (!input) throw new Error("Composer unavailable");
    input.value = text;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await Promise.resolve();
    return input;
  }
  const submit = (input: HTMLTextAreaElement) =>
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  const event = (payload: unknown) => tauri.events.get("engine")?.({ payload });

  try {
    await vi.waitFor(() => expect(target.querySelector("textarea")?.disabled).toBe(false));
    await draft("/comp");
    await vi.waitFor(() => expect(target.textContent).toContain("[instructions]"));
    submit(await draft("/compact retain decisions"));
    await vi.waitFor(() => expect(commands).toContainEqual({ type: "compact", customInstructions: "retain decisions" }));
    expect(target.textContent).toContain("Compacting the conversation…");
    event({ type: "response", id: compactId, success: true, data: { summary: "Kept decisions" } });
    await vi.waitFor(() => expect(target.textContent).toContain("Conversation compacted."));
    await vi.waitFor(() => expect(target.querySelector("textarea")?.disabled).toBe(false));

    event({ type: "agent_start" });
    await vi.waitFor(() => expect(target.querySelector('[aria-label="Stop"]')).not.toBeNull());
    submit(await draft("/compact"));
    await vi.waitFor(() => expect(target.textContent).toContain("then compact"));
    expect(target.querySelector("textarea")?.value).toBe("/compact");
    expect(commands.filter((item) => item.type === "compact")).toHaveLength(1);
    expect(commands.some((item) => item.type === "prompt")).toBe(false);
  } finally {
    await unmount(app);
    target.remove();
    localStorage.clear();
    if (originalScrollTo) Object.defineProperty(HTMLElement.prototype, "scrollTo", originalScrollTo);
    else Reflect.deleteProperty(HTMLElement.prototype, "scrollTo");
  }
});

it("keeps Chat questions and video analysis in Chat and resumes Canvas on its board", async () => {
  localStorage.setItem("hibiscus.ownProvider", "1");
  const originalScrollTo = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "scrollTo");
  Object.defineProperty(HTMLElement.prototype, "scrollTo", { configurable: true, value: vi.fn() });

  const chatSession = { path: "/sessions/chat.jsonl", title: "Chat example", kind: "chat" };
  const canvasSession = { path: "/sessions/canvas.jsonl", title: "Canvas example", kind: "canvas" };
  const sessions = [chatSession, canvasSession];
  const questions = [
    { question: "Which output?", options: ["JSON", "Plain text"] },
    { question: "Which language?", options: ["English", "Bahasa Melayu"] },
  ];
  const savedQuestion = [
    { role: "user", content: [{ type: "text", text: "Generate a report" }] },
    { role: "assistant", content: [{ type: "toolCall", id: "ask-1", name: "ask_user", arguments: { questions } }] },
    { role: "toolResult", toolCallId: "ask-1", content: [], details: { questions } },
  ];
  const emit = (payload: unknown) => tauri.events.get("engine")?.({ payload });
  type Command = { type: string; message?: string; sessionPath?: string; kind?: string };
  const commands: Command[] = [];
  let current: (typeof sessions)[number] | undefined;
  let sequence = 0;

  tauri.invoke.mockImplementation(async (name: string, args?: { command?: Command; session?: string; kind?: string }) => {
    if (name === "open_project") {
      current = sessions.find((s) => s.path === args?.session);
      return false;
    }
    if (name === "sessions") return [...sessions];
    if (name === "wsl_distros" || name === "canvas_styles") return [];
    if (name === "video_engine_installed") return true;
    if (name === "send" && args?.command) {
      const command = args.command;
      commands.push(command);
      if (command.type === "switch_session") current = sessions.find((s) => s.path === command.sessionPath);
      if (command.type === "new_session") {
        current = { path: "/sessions/new.jsonl", title: "Untitled Canvas", kind: command.kind ?? "chat" };
        if (command.kind === "canvas") sessions.push(current);
      }
      const id = `desktop-session-kind-${++sequence}`;
      emit({
        type: "response", id, success: true,
        data: {
          hasAccess: true, levels: [], models: [],
          sessionFile: current?.path ?? null, sessionKind: current?.kind ?? "chat",
          messages: current?.kind === "chat" ? savedQuestion : [],
        },
      });
      if (command.type === "prompt") {
        emit({ type: "agent_start" });
        emit({ type: "message_start", message: { role: "user", content: [{ type: "text", text: command.message }] } });
      }
      return id;
    }
    return null;
  });

  const target = document.body.appendChild(document.createElement("div"));
  const app = mount(App, { target });
  const canvas = () => target.querySelector('.canvas[aria-label="Canvas"]');
  const form = () => target.querySelector('form[aria-label="Questions from Hibi"]');
  const button = (name: string) => Array.from(target.querySelectorAll("button")).find((el) => el.textContent?.trim() === name);

  try {
    await vi.waitFor(() => expect(button("Chat example")?.disabled).toBe(false));
    expect(target.querySelector('[aria-label="Chat sessions"]')?.textContent).toContain("Chat example");
    expect(target.querySelector('[aria-label="Canvas sessions"]')?.textContent).toContain("Canvas example");
    button("Chat example")?.click();
    await vi.waitFor(() => expect(button("Send answers")).toBeDefined());
    await vi.waitFor(() => expect(target.querySelector("textarea")?.disabled).toBe(false));

    emit({ type: "tool_execution_start", toolName: "analyze_video", toolCallId: "video-1", args: { path: "clip.mp4" } });
    emit({ type: "tool_execution_end", toolName: "analyze_video", toolCallId: "video-1", result: { details: { frames: [0] } } });
    await vi.waitFor(() => expect(button("Used 2 tools")).toBeDefined());
    button("Used 2 tools")?.click();
    await vi.waitFor(() => expect(target.textContent).toContain("clip.mp4"));
    expect(canvas()).toBeNull();
    expect(tauri.invoke.mock.calls.some(([name, args]) => name === "canvas_load" && args.session === chatSession.path)).toBe(false);
    expect(button("Send answers")?.disabled).toBe(true);
    button("Plain text")?.click();
    const language = target.querySelector<HTMLInputElement>('input[aria-label="Which language?"]')!;
    language.value = "French";
    language.dispatchEvent(new Event("input", { bubbles: true }));
    await vi.waitFor(() => expect(button("Send answers")?.disabled).toBe(false));
    form()?.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(commands).toContainEqual({
      type: "prompt", message: "My answers:\n- Which output?\n  Plain text\n- Which language?\n  French\n\nCarry on from here.",
    }));
    emit({ type: "agent_settled" });
    await vi.waitFor(() => expect(form()).toBeNull());

    button("Canvas example")?.click();
    await vi.waitFor(() => expect(canvas()).not.toBeNull());
    expect(tauri.invoke).toHaveBeenCalledWith("canvas_load", { session: canvasSession.path });
    emit({ type: "agent_start" });
    emit({ type: "tool_execution_start", toolName: "ask_user", toolCallId: "ask-2", args: { questions } });
    emit({ type: "tool_execution_end", toolName: "ask_user", toolCallId: "ask-2", result: { details: { questions } } });
    emit({ type: "agent_settled" });
    await vi.waitFor(() => expect(canvas()?.textContent).toContain("Which output?"));
    expect(form()).toBeNull();
    button("New Canvas session")?.click();
    await vi.waitFor(() => expect(commands).toContainEqual({ type: "new_session", kind: "canvas" }));
    await vi.waitFor(() => expect(target.querySelector("li.active")?.textContent).toContain("Untitled Canvas"));
    expect(canvas()?.textContent).not.toContain("Which output?");
  } finally {
    await unmount(app);
    target.remove();
    localStorage.clear();
    if (originalScrollTo) Object.defineProperty(HTMLElement.prototype, "scrollTo", originalScrollTo);
    else Reflect.deleteProperty(HTMLElement.prototype, "scrollTo");
  }
});

it("sends a local text reference after preparing a PDF attachment", async () => {
  localStorage.setItem("hibiscus.ownProvider", "1");
  let sequence = 0;
  const prompts: unknown[] = [];
  const file = new File(["%PDF-1.4"], "guide.pdf", { type: "application/pdf" });
  const bytes = new TextEncoder().encode("%PDF-1.4");
  Object.defineProperty(file, "arrayBuffer", { value: async () => bytes.buffer });
  const extractedTextPath = "/app/attachments/pdf-local.txt";
  tauri.invoke.mockImplementation(async (name: string, args?: { command?: { type: string } }) => {
    if (name === "open_project") return false;
    if (name === "sessions" || name === "wsl_distros" || name === "canvas_styles") return [];
    if (name === "prepare_pdf") return extractedTextPath;
    if (name === "send" && args?.command) {
      if (args.command.type === "prompt") prompts.push(args.command);
      const id = `desktop-pdf-${++sequence}`;
      tauri.events.get("engine")?.({ payload: {
        type: "response", id, success: true,
        data: { hasAccess: true, messages: [], models: [], levels: [] },
      } });
      return id;
    }
    return null;
  });
  const target = document.body.appendChild(document.createElement("div"));
  const app = mount(App, { target });

  try {
    await vi.waitFor(() => expect(target.querySelector("textarea")?.disabled).toBe(false));
    const input = target.querySelector("textarea")!;
    input.value = "Summarize page two";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    const drop = new Event("drop", { bubbles: true, cancelable: true });
    Object.defineProperty(drop, "dataTransfer", { value: { files: [file] } });
    input.dispatchEvent(drop);
    await vi.waitFor(() => expect(target.textContent).toContain("guide.pdf"));
    target.querySelector<HTMLButtonElement>('[aria-label="Send"]')?.click();
    await vi.waitFor(() => expect(prompts).toHaveLength(1));
    const payload = tauri.invoke.mock.calls.filter(([name]) => name === "prepare_pdf").at(-1)?.[1];
    expect(Array.from(payload ?? [])).toEqual(Array.from(bytes));
    expect(prompts[0]).toMatchObject({
      message: expect.stringContaining(JSON.stringify([{ name: "guide.pdf", extractedTextPath }])),
      images: [],
    });
    expect(JSON.stringify(prompts[0])).not.toContain("%PDF-");
  } finally {
    await unmount(app);
    target.remove();
    localStorage.clear();
  }
});
