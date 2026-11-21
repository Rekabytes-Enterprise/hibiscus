import { beforeEach, describe, expect, it, vi } from "vitest";
import { switchProjectSession, type Record } from "./engine";

const tauri = vi.hoisted(() => ({
  invoke: vi.fn(),
  events: new Map<string, (event: { payload: Record }) => void>(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, handler: (event: { payload: Record }) => void) => {
    tauri.events.set(name, handler);
    return Promise.resolve(() => {});
  },
}));

function reply(success: boolean, error?: string) {
  tauri.events.get("engine")?.({
    payload: { type: "response", id: "desktop-1", success, error },
  });
}

beforeEach(() => tauri.invoke.mockReset());

describe("desktop session switching", () => {
  it.each(["No project is open.", "The engine has stopped."])(
    "opens the selected session when the worker reports %s",
    async (error) => {
      tauri.invoke.mockRejectedValueOnce(error).mockResolvedValueOnce(false);
      await expect(switchProjectSession("/work", "/sessions/next.jsonl")).resolves.toBe(false);
      expect(tauri.invoke).toHaveBeenNthCalledWith(2, "open_project", {
        folder: "/work", latest: false, session: "/sessions/next.jsonl",
      });
    },
  );

  it("creates a new session in the same folder when replacing a dead worker", async () => {
    tauri.invoke.mockRejectedValueOnce("No project is open.").mockResolvedValueOnce(false);
    await expect(switchProjectSession("/work")).resolves.toBe(false);
    expect(tauri.invoke).toHaveBeenNthCalledWith(1, "send", { command: { type: "new_session" } });
    expect(tauri.invoke).toHaveBeenNthCalledWith(2, "open_project", {
      folder: "/work", latest: false, session: null,
    });
  });

  it("keeps the current worker when the requested saved session is missing", async () => {
    tauri.invoke.mockImplementationOnce(async () => {
      reply(false, "Session file was removed");
      return "desktop-1";
    });
    await expect(switchProjectSession("/work", "/sessions/gone.jsonl")).rejects.toThrow("Session file was removed");
    expect(tauri.invoke).toHaveBeenCalledTimes(1);
  });

  it("reuses a healthy worker", async () => {
    tauri.invoke.mockImplementationOnce(async () => {
      reply(true);
      return "desktop-1";
    });
    await expect(switchProjectSession("/work", "/sessions/next.jsonl")).resolves.toBe(true);
    expect(tauri.invoke).toHaveBeenCalledTimes(1);
  });
});
