import { expect, it } from "vitest";
import { empty, type Item } from "./chat";
import type { Job } from "./engine";
import { tasks, took } from "./tasks";

const tool = (id: string, args: any, extra: Partial<Item> = {}): Item => ({
  kind: "tool", id, name: "bash", args, running: false, error: false, output: "", diff: "", image: "", trimmed: false, pendingImage: false, ms: 1000, ...extra,
} as Item);

const job = (id: number, extra: Partial<Job> = {}): Job => ({
  id, label: `job ${id}`, command: "sleep 9", started: 0, running: false, exitCode: 0, stopped: false, durationMs: 9000, output: ["a", "b"], ...extra,
});

it("lists running tasks apart from finished ones, jobs first, newest command first", () => {
  const chat = {
    ...empty(),
    items: [
      tool("1", { command: "ping -c 3 rkb", description: "Ping rkb" }),
      tool("2", { command: "npm run render", background: true }),
      tool("3", { command: "cargo test\n--all" }, { running: true, ms: null }),
      { kind: "assistant", text: "done", streaming: false } as Item,
      tool("4", { command: "false" }, { error: true, output: "exit 1" }),
    ],
  };
  const { running, finished } = tasks(chat, [job(7, { running: true }), job(6, { exitCode: 1 }), job(5, { stopped: true, exitCode: null })]);
  expect(running.map((t) => t.key)).toEqual(["job-7", "3"]);
  expect(running[1].label).toBe("cargo test");
  expect(finished.map((t) => t.key)).toEqual(["job-6", "job-5", "4", "1"]);
  expect(finished[0].failed).toBe(true);
  expect(finished[1]).toMatchObject({ stopped: true, failed: false, output: "a\nb", job: 5 });
  expect(finished[3].label).toBe("Ping rkb");
});

it("formats how long a task took", () => {
  expect(took(4600)).toBe("5s");
  expect(took(113_000)).toBe("1m 53s");
  expect(took(975_000)).toBe("16m 15s");
});
