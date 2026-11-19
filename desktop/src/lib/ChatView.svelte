<script lang="ts">
  import { tick } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { entries, summary, type Chat } from "./chat";
  import Icon from "./Icon.svelte";
  import ToolCall from "./ToolCall.svelte";
  import { cachedMarkdown, markdown } from "./markdown";

  let {
    chat,
    folder,
    onolder,
    onresend,
  }: {
    chat: Chat;
    folder: string | null;
    /** Loads the page of messages before the loaded ones, when `chat.start` says there are more. */
    onolder: () => Promise<void>;
    onresend: (text: string) => void;
  } = $props();
  /** Groups the user opened, by the id of their first tool call. */
  let opened = $state<{ [id: string]: boolean }>({});
  let scroller: HTMLDivElement;
  let pinned = true;

  // A long session renders its newest entries first and the older ones as the
  // user scrolls up (#159). Once all loaded entries show, the next page of
  // messages comes from the engine (#171). Keys are positions in the whole
  // list less the entries loaded above since, so rows already on screen keep
  // their keys.
  const PAGE = 50;
  let shown = $state(PAGE);
  let above = $state(0);
  let loading = false;
  const all = $derived(entries(chat.items));
  const start = $derived(Math.max(0, all.length - shown));

  async function older() {
    if (loading || (start === 0 && !chat.start)) return;
    loading = true;
    const height = scroller.scrollHeight;
    if (start === 0) {
      const before = all.length;
      await onolder();
      above += entries(chat.items).length - before;
    }
    shown += PAGE;
    await tick();
    // Keep the entry the user was reading in place.
    scroller.scrollTop += scroller.scrollHeight - height;
    loading = false;
  }

  // Follow new output unless the user scrolled up to read.
  $effect(() => {
    chat.items;
    if (pinned) tick().then(() => scroller?.scrollTo({ top: scroller.scrollHeight }));
  });

  // A followed link would replace the app itself; send web links to the browser instead.
  function onclick(e: MouseEvent) {
    const copy = (e.target as Element).closest?.(".copycode");
    if (copy) {
      const code = copy.parentElement?.querySelector("pre")?.textContent ?? "";
      void navigator.clipboard.writeText(code).then(() => {
        copy.classList.add("copied");
        setTimeout(() => copy.classList.remove("copied"), 1500);
      });
      return;
    }
    const link = (e.target as Element).closest?.("a[href]");
    if (!link) return;
    e.preventDefault();
    const href = link.getAttribute("href") ?? "";
    if (/^https?:\/\//i.test(href)) void openUrl(href).catch(() => {});
  }

  function onscroll() {
    pinned = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 40;
    if (scroller.scrollTop < 200) void older();
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="chat" bind:this={scroller} {onscroll} {onclick}>
  {#if start > 0 || chat.start}
    <button class="link earlier" onclick={older}>Show earlier messages</button>
  {/if}
  {#each all.slice(start) as item, i (start + i - above)}
    {#if item.kind === "user"}
      <div class="user">{item.text}{#if item.images}<span class="muted small"> · {item.images} image{item.images > 1 ? "s" : ""}</span>{/if}</div>
    {:else if item.kind === "assistant"}
      <div class="assistant md" class:streaming={item.streaming}>{@html item.streaming ? markdown(item.text) : cachedMarkdown(item.text)}</div>
    {:else if item.kind === "tool"}
      <ToolCall tool={item} {folder} />
    {:else if item.kind === "tools"}
      {@const id = item.tools[0].id}
      {@const failed = item.tools.some((t) => t.error)}
      {@const open = opened[id] ?? failed}
      <div class="toolgroup">
        <button class="toolrow" onclick={() => (opened[id] = !open)} aria-expanded={open}>
          {#if item.tools.some((t) => t.running)}<span class="toolicon"><span class="spinner"></span></span>{/if}
          <span class="verb">{summary(item.tools)}</span>
          <Icon name={open ? "chevron-down" : "chevron-right"} size={12} />
        </button>
        {#if open}
          <div class="toolchildren">
            {#each item.tools as tool (tool.id)}<ToolCall {tool} {folder} />{/each}
          </div>
        {/if}
      </div>
    {:else}
      <div class="notice" class:error={item.error}>{item.text}</div>
    {/if}
  {:else}
    <div class="empty muted">Ask Hibiscus to read, change or explain code in this folder.</div>
  {/each}
  {#if chat.running}
    <div class="muted small working">{chat.waiting ? "Waiting in the Rooxcode queue…" : "Working…"}</div>
  {/if}
  {#each chat.rejected as text}
    <div class="notice error">Not delivered: “{text}” <button class="link" onclick={() => onresend(text)}>Send again</button></div>
  {/each}
</div>
