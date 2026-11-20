<script lang="ts">
  import { onDestroy } from "svelte";
  import type { Chat } from "./chat";
  import { clearJobs, listJobs, stopJob, type Job } from "./engine";
  import Icon from "./Icon.svelte";
  import { tasks, took, type Task } from "./tasks";

  /** The agent's background jobs and the commands it ran in this session. */
  let {
    folder,
    chat,
    wide,
    onwide,
    onclose,
    onabort,
  }: {
    folder: string;
    chat: Chat;
    wide: boolean;
    onwide: () => void;
    onclose: () => void;
    /** Stops the agent's run, for a command it is waiting on. */
    onabort: () => void;
  } = $props();

  let jobs = $state<Job[]>([]);
  // Running tasks start open, so their output shows as it comes.
  let open = $state<Record<string, boolean>>({});
  let finishedOpen = $state(true);

  const list = $derived(tasks(chat, jobs));
  const isOpen = (t: Task) => open[t.key] ?? t.running;

  async function poll() {
    jobs = await listJobs(folder).catch(() => jobs);
  }
  void poll();
  const timer = setInterval(poll, 1000);
  onDestroy(() => clearInterval(timer));

  async function stop(t: Task) {
    if (t.job === undefined) return onabort();
    await stopJob(t.job).catch(() => {});
    await poll();
  }

  async function clear() {
    await clearJobs(folder).catch(() => {});
    await poll();
  }

  const status = (t: Task) => (t.running ? (t.ms != null ? took(t.ms) : "Running") : t.stopped ? `Stopped · ${took(t.ms ?? 0)}` : t.failed ? `Failed${t.ms != null ? ` · ${took(t.ms)}` : ""}` : t.ms != null ? took(t.ms) : "");
</script>

{#snippet row(t: Task)}
  <section class="task" class:failed={t.failed}>
    <div class="taskhead">
      <button class="name" onclick={() => (open[t.key] = !isOpen(t))} aria-expanded={isOpen(t)} title={t.command}>
        <Icon name={isOpen(t) ? "chevron-down" : "chevron-right"} size={12} />
        {#if t.running}<span class="spinner" aria-hidden="true"></span>{/if}
        <span class="grow ellipsis">{t.label || "Command"}</span>
        <span class="muted small">{status(t)}</span>
      </button>
      {#if t.running}
        <button class="iconbtn" onclick={() => stop(t)} title={t.job === undefined ? "Stop the run" : "Stop this job"} aria-label="Stop"><Icon name="square" size={12} /></button>
      {/if}
    </div>
    {#if isOpen(t)}
      <pre class="command">$ {t.command}</pre>
      {#if t.output}<pre class="output">{t.output}</pre>{/if}
    {/if}
  </section>
{/snippet}

<aside class="diff tasks" class:wide>
  <header>
    <span class="grow">Background tasks</span>
    <button class="iconbtn" onclick={clear} title="Clear finished background jobs" aria-label="Clear finished background jobs"><Icon name="trash-2" size={14} /></button>
    <button class="iconbtn" onclick={onwide} title={wide ? "Shrink" : "Expand"} aria-label={wide ? "Shrink" : "Expand"}><Icon name={wide ? "minimize" : "maximize"} size={14} /></button>
    <button class="iconbtn" onclick={onclose} title="Close" aria-label="Close background tasks"><Icon name="x" size={14} /></button>
  </header>
  <div class="files">
    {#if !list.running.length && !list.finished.length}
      <div class="note">No commands yet. Commands the agent runs, and long ones it starts in the background, show here.</div>
    {/if}
    {#if list.running.length}
      <div class="groupname small muted">Running {list.running.length}</div>
      {#each list.running as t (t.key)}{@render row(t)}{/each}
    {/if}
    {#if list.finished.length}
      <button class="groupname small muted" onclick={() => (finishedOpen = !finishedOpen)} aria-expanded={finishedOpen}>
        <Icon name={finishedOpen ? "chevron-down" : "chevron-right"} size={12} /> Finished {list.finished.length}
      </button>
      {#if finishedOpen}
        {#each list.finished as t (t.key)}{@render row(t)}{/each}
      {/if}
    {/if}
  </div>
</aside>
