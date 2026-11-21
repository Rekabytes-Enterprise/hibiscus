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
