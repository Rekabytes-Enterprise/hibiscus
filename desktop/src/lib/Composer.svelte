<script lang="ts">
  import type { Chat } from "./chat";
  import Icon from "./Icon.svelte";

  type Image = { mimeType: string; data: string };
  let {
    running,
    chat,
    onsend,
    onabort,
    disabled = false,
  }: {
    running: boolean;
    /** While a session loads, so nothing is sent to the wrong one. */
    disabled?: boolean;
    chat: Chat;
    onsend: (text: string, images: Image[], steer: boolean) => void;
    onabort: () => void;
  } = $props();

  let text = $state("");
  let images = $state<Image[]>([]);
  // While a run is active: steer = delivered before its next step, else after it finishes.
  let steer = $state(false);

  function submit() {
    const message = text.trim();
    if (disabled) return;
    if (!message && !images.length) return;
    onsend(message, images, steer);
    text = "";
    images = [];
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      submit();
    } else if (e.key === "Escape" && running) {
      onabort();
    }
  }

  let picker: HTMLInputElement;
  let dragging = $state(false);

  /** Adds image files; anything else is ignored. */
  async function attach(files: Iterable<File>) {
    for (const file of files) {
      if (!file.type.startsWith("image/")) continue;
      const url = await new Promise<string>((done) => {
        const reader = new FileReader();
        reader.onload = () => done(String(reader.result));
        reader.readAsDataURL(file);
      });
      images = [...images, { mimeType: file.type, data: url.slice(url.indexOf(",") + 1) }];
    }
  }

  function onpaste(e: ClipboardEvent) {
    const files = Array.from(e.clipboardData?.files ?? []);
    if (!files.some((f) => f.type.startsWith("image/"))) return;
    e.preventDefault();
    void attach(files);
  }

  function ondrop(e: DragEvent) {
    e.preventDefault();
    dragging = false;
    void attach(Array.from(e.dataTransfer?.files ?? []));
  }
</script>

<div class="composer" class:dragging role="group" aria-label="Message" ondragover={(e) => { e.preventDefault(); dragging = true; }} ondragleave={() => (dragging = false)} {ondrop}>
  {#if chat.steering.length || chat.followUp.length}
    <div class="muted small">Queued: {[...chat.steering, ...chat.followUp].join(" · ")}</div>
  {/if}
  {#if images.length}
    <div class="thumbs">
      {#each images as image, i}
        <div class="thumb">
          <img src={`data:${image.mimeType};base64,${image.data}`} alt={`Attached image ${i + 1}`} />
          <button class="remove" onclick={() => (images = images.filter((_, j) => j !== i))} aria-label={`Remove image ${i + 1}`} title="Remove"><Icon name="x" size={12} /></button>
        </div>
      {/each}
    </div>
  {/if}
  <div class="inputrow">
    <button class="plain attach" onclick={() => picker.click()} {disabled} title="Attach images" aria-label="Attach images"><Icon name="paperclip" size={16} /></button>
    <input bind:this={picker} type="file" accept="image/*" multiple hidden onchange={(e) => { void attach(Array.from(e.currentTarget.files ?? [])); e.currentTarget.value = ""; }} />
    <textarea
      bind:value={text}
      {onkeydown}
      {onpaste}
      rows="1"
      {disabled}
      placeholder={running ? "Queue a message for when this step finishes…" : "Describe a task or ask a question"}
    ></textarea>
    {#if running}<button class="send" onclick={onabort} title="Stop (Esc)" aria-label="Stop"><Icon name="square" size={14} /></button>{/if}
    <button class="primary send" onclick={submit} disabled={disabled || (!text.trim() && !images.length)} title={running ? "Queue" : "Send (Enter)"} aria-label={running ? "Queue" : "Send"}><Icon name="arrow-up" /></button>
  </div>
  {#if running}
    <label class="small muted"><input type="checkbox" bind:checked={steer} /> Steer the current run instead of waiting for it</label>
  {/if}
</div>
