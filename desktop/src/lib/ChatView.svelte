<script lang="ts">
  import { tick } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { entries, summary, type Chat } from "./chat";
  import Icon from "./Icon.svelte";
  import ToolCall from "./ToolCall.svelte";
  import { markdown } from "./markdown";

  let { chat, folder, onresend }: { chat: Chat; folder: string | null; onresend: (text: string) => void } = $props();
  /** Groups the user opened, by the id of their first tool call. */
  let opened = $state<{ [id: string]: boolean }>({});
  let scroller: HTMLDivElement;
  let pinned = true;

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
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="chat" bind:this={scroller} {onscroll} {onclick}>
  {#each entries(chat.items) as item}
    {#if item.kind === "user"}
      <div class="user">{item.text}{#if item.images}<span class="muted small"> · {item.images} image{item.images > 1 ? "s" : ""}</span>{/if}</div>
    {:else if item.kind === "assistant"}
      <div class="assistant md" class:streaming={item.streaming}>{@html markdown(item.text)}</div>
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
