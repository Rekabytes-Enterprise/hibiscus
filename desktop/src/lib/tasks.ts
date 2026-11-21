// What the Background tasks pane lists: the agent's background jobs and the
// commands it ran in this session. Pure, so it is unit-tested.
import type { Chat } from "./chat";
import type { Job } from "./engine";

export type Task = {
  key: string;
  label: string;
  command: string;
  output: string;
  running: boolean;
  /** How long it ran, when known. */
  ms: number | null;
  failed: boolean;
  stopped: boolean;
  /** Set for a background job, which can be stopped on its own. */
  job?: number;
};

/** A command's label: its description, or its first line. */
const label = (description: unknown, command: string) =>
  (typeof description === "string" && description.trim()) || command.split("\n")[0].trim();

/** Running tasks first, then finished ones; background jobs before the session's commands, newest first. */
export function tasks(chat: Chat, jobs: Job[]): { running: Task[]; finished: Task[] } {
  const fromJobs: Task[] = jobs.map((j) => ({
    key: `job-${j.id}`,
    label: j.label,
    command: j.command,
    output: j.output.join("\n"),
    running: j.running,
    ms: j.durationMs,
    failed: !j.running && !j.stopped && j.exitCode !== 0,
    stopped: j.stopped,
    job: j.id,
  }));
  // A background start returns at once; its job carries the command from there.
  const commands: Task[] = chat.items
    .flatMap((i) => (i.kind === "tool" && i.name === "bash" && !i.args?.background ? [i] : []))
    .reverse()
    .map((t) => ({
      key: t.id,
      label: label(t.args?.description, String(t.args?.command ?? "")),
      command: String(t.args?.command ?? ""),
      output: t.output,
      running: t.running,
      ms: t.ms,
      failed: t.error,
      stopped: false,
    }));
  const all = [...fromJobs, ...commands];
  return { running: all.filter((t) => t.running), finished: all.filter((t) => !t.running) };
}

/** "5s", "1m 53s", "16m 15s". */
export function took(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${s % 60}s`;
}
