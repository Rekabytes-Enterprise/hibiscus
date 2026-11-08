<script lang="ts">
  import Icon from "./Icon.svelte";
  import Providers from "./Providers.svelte";

  let { onback, onsaved }: { onback: () => void; onsaved: () => Promise<void> } = $props();
</script>

<div class="settings">
  <nav>
    <button class="nav" onclick={onback}><Icon name="arrow-left" /> Back</button>
    <div class="heading muted small">Settings</div>
    <button class="nav active"><Icon name="key" /> Providers</button>
  </nav>
  <section>
    <Providers {onsaved} />
  </section>
</div>
