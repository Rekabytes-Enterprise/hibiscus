<script lang="ts">
  import Icon from "./Icon.svelte";
  import { TERMS } from "./legal";
  import { markdown } from "./markdown";
  import Privacy from "./Privacy.svelte";
  import Providers from "./Providers.svelte";

  let { onback, onsaved }: { onback: () => void; onsaved: () => Promise<void> } = $props();
  let page = $state<"providers" | "privacy" | "terms">("providers");
</script>

<div class="settings">
  <nav>
    <button class="nav" onclick={onback}><Icon name="arrow-left" /> Back</button>
    <div class="heading muted small">Settings</div>
    <button class="nav" class:active={page === "providers"} onclick={() => (page = "providers")}><Icon name="key" /> Providers</button>
    <button class="nav" class:active={page === "privacy"} onclick={() => (page = "privacy")}><Icon name="shield" /> Privacy</button>
    <button class="nav" class:active={page === "terms"} onclick={() => (page = "terms")}><Icon name="file-text" /> Terms of Use</button>
  </nav>
  <section>
    {#if page === "providers"}
      <Providers {onsaved} />
    {:else if page === "privacy"}
      <Privacy />
    {:else}
      <h2>Terms of Use</h2>
      <div class="md legal">{@html markdown(TERMS)}</div>
    {/if}
  </section>
</div>
