<script lang="ts">
  import type { Chat } from "./chat";
  import Icon from "./Icon.svelte";
  import Menu from "./Menu.svelte";

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
  const sendModes = [
    { value: "queue", label: "Queue for next turn", hint: "Send when the current run finishes" },
    { value: "steer", label: "Steer current run", hint: "Send at the next safe point" },
  ] as const;
  type SendMode = (typeof sendModes)[number]["value"];
  let sendMode = $state<SendMode>("queue");
  const sendLabel = $derived(sendMode === "steer" ? "Steer" : "Queue");
  const canSend = $derived(!disabled && (!!text.trim() || images.length > 0));

  $effect(() => {
    if (!running || disabled) sendMode = "queue";
  });

  let input: HTMLTextAreaElement;
  const suggestCompact = $derived(text.startsWith("/") && "/compact".startsWith(text));

  function chooseCompact() {
    text = "/compact ";
    input.focus();
  }

  /** Adds text and images from elsewhere in the app, such as browser comments, for the user to send. */
  export function add(more: string, moreImages: Image[]) {
    text = text.trim() ? `${text.trim()}\n\n${more}` : more;
    images = [...images, ...moreImages];
    input?.focus();
  }

  function submit() {
    if (!canSend) return;
    const message = text.trim();
    const attachments = images;
    const steer = running && sendMode === "steer";
    text = "";
    images = [];
    sendMode = "queue";
    onsend(message, attachments, steer);
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
  {#if suggestCompact && !disabled}
    <div class="composer-commands">
      <button class="plain" type="button" onclick={chooseCompact}><code>/compact</code> <span class="muted">[instructions]</span></button>
    </div>
  {/if}
  <div class="inputrow">
    <button class="plain attach" onclick={() => picker.click()} {disabled} title="Attach images" aria-label="Attach images"><Icon name="paperclip" size={16} /></button>
    <input bind:this={picker} type="file" accept="image/*" multiple hidden onchange={(e) => { void attach(Array.from(e.currentTarget.files ?? [])); e.currentTarget.value = ""; }} />
    <textarea
      bind:this={input}
      bind:value={text}
      {onkeydown}
      {onpaste}
      rows="1"
      {disabled}
      placeholder={running ? "Message Hibi…" : "Describe a task or ask a question"}
    ></textarea>
    {#if running}
      <button class="send" onclick={onabort} title="Stop (Esc)" aria-label="Stop"><Icon name="square" size={14} /></button>
      {#key disabled}
        <div class="send-split">
          <button class="primary send" onclick={submit} disabled={!canSend} title={sendMode === "steer" ? "Steer current run (Enter)" : "Queue for next turn (Enter)"}>{sendLabel}</button>
          <Menu
            options={[...sendModes]}
            value={sendMode}
            label="Send mode"
            onpick={(mode) => { sendMode = mode; input.focus(); }}
            {disabled}
          >
            {#snippet trigger()}{/snippet}
          </Menu>
        </div>
      {/key}
    {:else}
      <button class="primary send" onclick={submit} disabled={!canSend} title="Send (Enter)" aria-label="Send"><Icon name="arrow-up" /></button>
    {/if}
  </div>
</div>

<style>
  textarea { min-width: 0; }
  .send-split { display: flex; flex: none; }
  .send-split .send { width: auto; min-width: 56px; padding: 0 9px; border-radius: 8px 0 0 8px; font-size: 12px; font-weight: 600; }
  .send-split :global(.picker) {
    height: 32px; width: 26px; justify-content: center; padding: 0;
    color: var(--accent-text); background: var(--accent); border-radius: 0 8px 8px 0;
    border-left: 1px solid color-mix(in srgb, var(--accent-text) 22%, transparent);
  }
  .send-split .send:hover:not(:disabled),
  .send-split .send:focus-visible,
  .send-split :global(.picker:hover:not(:disabled)),
  .send-split :global(.picker:focus-visible),
  .send-split :global(.picker[aria-expanded="true"]) {
    background: color-mix(in srgb, var(--accent) 86%, var(--accent-text));
  }
  .send-split :global(.popover) { right: 0; left: auto; }
</style>
