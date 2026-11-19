<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { RELEASES, type Release } from "./changelog";
  import Icon from "./Icon.svelte";
  import { markdown } from "./markdown";

  const REPO = "https://github.com/Rekabytes-Enterprise/hibiscus";
  const [latest, ...earlier] = RELEASES;
  const installed = getVersion().catch(() => null);

  const count = (r: Release) => r.sections.reduce((n, s) => n + s.items.length, 0);
  const tag = (title: string) => title.toLowerCase().replace(/[^a-z]/g, "");
  const day = (date: string | null) =>
    date ? new Date(date + "T00:00:00").toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" }) : "";
  const open = (version: string) => openUrl(`${REPO}/releases/tag/v${version}`).catch(() => {});
</script>

{#snippet notes(r: Release)}
  {#each r.sections as s}
    <div class="notes">
      <span class="tag {tag(s.title)}">{s.title}</span>
      <ul>
        {#each s.items as item}<li class="md">{@html markdown(item)}</li>{/each}
      </ul>
    </div>
  {/each}
{/snippet}

<div class="about">
  <h2>About</h2>
  {#if latest}
    <div class="bento">
      <article class="tile hero">
        <div class="eyebrow"><Icon name="zap" size={14} /> Latest release</div>
        <div class="headline">
          <span class="ver">v{latest.version}</span>
          <span class="when">{day(latest.date)}</span>
        </div>
        {@render notes(latest)}
      </article>

      <article class="tile">
        <div class="eyebrow muted">Installed</div>
        {#await installed then v}
          <div class="big">{v ? `v${v}` : "Unknown"}</div>
          {#if v === latest.version}
            <span class="pill ok"><Icon name="check" size={12} /> Up to date</span>
          {:else if v}
            <span class="pill">v{latest.version} is out</span>
          {/if}
        {/await}
        <button class="linkbtn" onclick={() => open(latest.version)}>Release notes on GitHub <Icon name="arrow-up-right" size={14} /></button>
      </article>

      <article class="tile">
        <div class="eyebrow muted">In this release</div>
        <div class="big">{count(latest)} <span class="unit">{count(latest) === 1 ? "change" : "changes"}</span></div>
        <div class="chips">
          {#each latest.sections as s}<span class="tag {tag(s.title)}">{s.items.length} {s.title.toLowerCase()}</span>{/each}
        </div>
        <div class="muted small">{RELEASES.length} releases in total</div>
      </article>
    </div>

    {#if earlier.length}
      <h3>Earlier releases</h3>
      <div class="releases">
        {#each earlier as r (r.version)}
          <details class="tile release">
            <summary>
              <span class="meta grow">
                <span class="ver">v{r.version}</span>
                <span class="muted small">{[day(r.date), `${count(r)} ${count(r) === 1 ? "change" : "changes"}`].filter(Boolean).join(" · ")}</span>
              </span>
              <span class="chevron"><Icon name="chevron-down" size={14} /></span>
            </summary>
            <div class="body">{@render notes(r)}</div>
          </details>
        {/each}
      </div>
    {/if}
  {/if}
</div>
