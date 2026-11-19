<script lang="ts">
  import { onMount } from "svelte";
  import { setTrainingDenied, trainingDenied } from "./engine";
  import { PRIVACY } from "./legal";
  import { markdown } from "./markdown";

  let deny = $state(false);
  let saving = $state(false);
  let error = $state<string | null>(null);

  onMount(async () => {
    deny = await trainingDenied().catch(() => false);
  });

  async function toggle(next: boolean) {
    saving = true;
    error = null;
    try {
      await setTrainingDenied(next);
      deny = next;
    } catch (e) {
      error = String(e instanceof Error ? e.message : e);
    } finally {
      saving = false;
    }
  }
</script>

<h2>Privacy</h2>
<ul class="settings-list">
  <li>
    <label class="toggle">
      <span class="grow">
        <span>Don't let providers train on my prompts</span>
        <span class="muted small">OpenRouter only uses providers that don't store or train on your prompts. Some free models may become unavailable. Applies to the terminal app too.</span>
      </span>
      <input type="checkbox" role="switch" checked={deny} disabled={saving} onchange={(e) => toggle(e.currentTarget.checked)} />
    </label>
  </li>
</ul>
{#if error}<p class="error small">{error}</p>{/if}
<h3>Privacy Policy</h3>
<div class="md legal">{@html markdown(PRIVACY)}</div>
