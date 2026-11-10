<script lang="ts" generics="T">
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";

  /**
   * An in-app dropdown that opens above its button, in place of a native `select`.
   * With `actions`, it is a list of commands behind an icon button instead, placed
   * on the screen so a scrolling list can't clip it.
   */
  let {
    options,
    value,
    label,
    onpick,
    disabled = false,
    title,
    trigger,
    actions = false,
  }: {
    options: { value: T; label: string; hint?: string }[];
    value?: T;
    label: string;
    onpick: (value: T) => void;
    disabled?: boolean;
    title?: string;
    trigger: Snippet;
    actions?: boolean;
  } = $props();

  let open = $state(false);
  let root: HTMLElement;
  let place = $state("");

  function toggle(e: MouseEvent) {
    if (actions && !open) {
      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
      place = `top: ${r.bottom + 4}px; left: ${Math.max(8, r.right - 180)}px;`;
    }
    open = !open;
  }

  function pick(v: T) {
    open = false;
    onpick(v);
  }

  function outside(e: PointerEvent) {
    if (open && !root.contains(e.target as Node)) open = false;
  }
</script>

<svelte:window onpointerdown={outside} onkeydown={(e) => { if (open && e.key === "Escape") open = false; }} />

<div class="menu" bind:this={root}>
  <button class={actions ? "iconbtn more" : "plain picker"} {disabled} {title} aria-haspopup="menu" aria-expanded={open} aria-label={label} onclick={toggle}>
    {@render trigger()}
    {#if !actions}<Icon name="chevron-down" size={12} />{/if}
  </button>
  {#if open}
    <div class="popover" class:fixed={actions} style={actions ? place : undefined} role="menu" aria-label={label}>
      {#each options as o}
        {#if actions}
          <button role="menuitem" onclick={() => pick(o.value)} title={o.hint}>{o.label}</button>
        {:else}
        <button role="menuitemradio" aria-checked={o.value === value} onclick={() => pick(o.value)} title={o.hint}>
          <span class="tick">{#if o.value === value}<Icon name="check" size={14} />{/if}</span>
          <span class="grow ellipsis">{o.label}</span>
        </button>
        {/if}
      {/each}
    </div>
  {/if}
</div>
