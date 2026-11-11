<script lang="ts">
  import { toolHunks, toolView, type Tool } from "./chat";
  import Hunks from "./Hunks.svelte";
  import Icon from "./Icon.svelte";

  let { tool, folder }: { tool: Tool; folder: string | null } = $props();

  const view = $derived(toolView(tool, folder));
  const hunks = $derived(tool.error ? [] : toolHunks(tool));
  // A read's contents are not worth showing; only its error is.
  const foldable = $derived(tool.name !== "read" || tool.error);
  /** What the copy button copies: the new text for edits and writes, else the output. */
  const copyable = $derived(
    tool.error ? tool.output : tool.name === "edit" ? tool.args?.newText : tool.name === "write" ? tool.args?.content : tool.output,
  );
  let copied = $state(false);
  async function copy() {
    await navigator.clipboard.writeText(String(copyable ?? ""));
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }
  // The screenshot shown full size over the app, until a click or Esc.
  let zoomed = $state(false);
  // Failed calls open by themselves so the error shows.
  let open = $state(false);
  $effect(() => {
    if (tool.error) open = true;
  });
</script>

<svelte:window onkeydown={(e) => zoomed && e.key === "Escape" && (zoomed = false)} />

<div class="toolcall" class:failed={tool.error}>
  <button class="toolrow" onclick={() => foldable && (open = !open)} aria-expanded={foldable ? open : undefined} class:static={!foldable}>
    <span class="toolicon">
      {#if tool.running}<span class="spinner"></span>{:else if tool.error}<Icon name="x" size={14} />{:else}<Icon name={view.icon as any} size={14} />{/if}
    </span>
    <span class="verb">{view.verb}</span>
    {#if view.target}<span class="target ellipsis" class:mono={view.command} title={view.target}>{view.target}</span>{/if}
    {#if foldable}<Icon name={open ? "chevron-down" : "chevron-right"} size={12} />{/if}
    <span class="grow"></span>
    {#if tool.ms != null && tool.ms >= 100}<span class="ms">{(tool.ms / 1000).toFixed(1)}s</span>{/if}
  </button>
  {#if tool.image}
    <button class="shot" onclick={() => (zoomed = true)} title="Show full size" aria-label="Show the screenshot full size"><img src={tool.image} alt="What Hibi saw" /></button>
  {/if}
  {#if zoomed}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="backdrop lightbox" onclick={() => (zoomed = false)}><img src={tool.image} alt="What Hibi saw" /></div>
  {/if}
  {#if open && foldable}
    <div class="toolbody">
      {#if view.command}<pre class="cmd">$ {tool.args?.command ?? ""}</pre>{/if}
      <div class="toolbox">
        {#if hunks.length}
          <div class="toolscroll"><Hunks {hunks} /></div>
        {:else if tool.output || tool.running}
          <pre class="toolscroll">{tool.output || "Running…"}</pre>
        {:else}
          <div class="muted small">No output</div>
        {/if}
        {#if copyable && !tool.running}
          <button class="iconbtn copy" onclick={copy} title={copied ? "Copied" : "Copy"} aria-label="Copy"><Icon name={copied ? "check" : "copy"} size={13} /></button>
        {/if}
      </div>
    </div>
  {/if}
</div>
