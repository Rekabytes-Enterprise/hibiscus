<script lang="ts">
  import { gapBefore, type Hunk } from "./diff";

  /** Diff lines with old and new line numbers, as in the Changes pane. */
  let { hunks }: { hunks: Hunk[] } = $props();
</script>

<table class="hunks">
  <!-- Fixed layout sizes columns from the first row, which can be a full-width gap row. -->
  <colgroup><col class="no" /><col class="no" /><col /></colgroup>
  <tbody>
    {#each hunks as h, i}
      {@const gap = gapBefore(hunks, i)}
      {#if gap}<tr class="gap"><td colspan="3">{gap} unchanged line{gap > 1 ? "s" : ""}</td></tr>{/if}
      {#each h.lines as l}
        <tr class={l.kind}>
          <td class="no">{l.old ?? ""}</td>
          <td class="no">{l.new ?? ""}</td>
          <td class="code">{l.text}</td>
        </tr>
      {/each}
    {/each}
  </tbody>
</table>
