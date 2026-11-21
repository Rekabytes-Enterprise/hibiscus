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
