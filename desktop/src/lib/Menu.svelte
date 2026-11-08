<script lang="ts" generics="T">
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";

  /** An in-app dropdown that opens above its button, in place of a native `select`. */
  let {
    options,
    value,
    label,
    onpick,
    disabled = false,
    title,
    trigger,
  }: {
    options: { value: T; label: string; hint?: string }[];
    value: T;
    label: string;
    onpick: (value: T) => void;
    disabled?: boolean;
    title?: string;
    trigger: Snippet;
  } = $props();

  let open = $state(false);
  let root: HTMLElement;

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
  <button class="plain picker" {disabled} {title} aria-haspopup="menu" aria-expanded={open} aria-label={label} onclick={() => (open = !open)}>
    {@render trigger()}
    <Icon name="chevron-down" size={12} />
  </button>
  {#if open}
    <div class="popover" role="menu" aria-label={label}>
      {#each options as o}
        <button role="menuitemradio" aria-checked={o.value === value} onclick={() => pick(o.value)} title={o.hint}>
          <span class="tick">{#if o.value === value}<Icon name="check" size={14} />{/if}</span>
          <span class="grow ellipsis">{o.label}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>
