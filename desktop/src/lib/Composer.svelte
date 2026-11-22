<script lang="ts">
  import type { Chat } from "./chat";
  import Icon from "./Icon.svelte";
  import Menu from "./Menu.svelte";

  type Image = { mimeType: string; data: string };
  let {
    running,
    chat,
    onsend,
    onpdfsend,
    onabort,
    disabled = false,
  }: {
    running: boolean;
    /** While a session loads, so nothing is sent to the wrong one. */
    disabled?: boolean;
    chat: Chat;
    onsend: (text: string, images: Image[], steer: boolean) => void;
    onpdfsend: (text: string, images: Image[], steer: boolean, pdfs: File[]) => Promise<void>;
    onabort: () => void;
  } = $props();

  let text = $state("");
  let images = $state<Image[]>([]);
  let pdfs = $state<File[]>([]);
  let preparing = $state(false);
  let pdfError = $state("");
  const blocked = $derived(disabled || preparing);
  const isPdf = (file: File) => file.type === "application/pdf" || /\.pdf$/i.test(file.name);

  const sendModes = [
    { value: "queue", label: "Queue for next turn", hint: "Send when the current run finishes" },
    { value: "steer", label: "Steer current run", hint: "Send at the next safe point" },
  ] as const;
  type SendMode = (typeof sendModes)[number]["value"];
  let sendMode = $state<SendMode>("queue");
  const sendLabel = $derived(sendMode === "steer" ? "Steer" : "Queue");
  const canSend = $derived(!blocked && (!!text.trim() || images.length > 0 || pdfs.length > 0));

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

  async function submitPdf(message: string, attachments: Image[], steer: boolean) {
    preparing = true;
    pdfError = "";
    try {
      await onpdfsend(message, attachments, steer, [...pdfs]);
      text = "";
      images = [];
      pdfs = [];
      sendMode = "queue";
    } catch (error) {
      pdfError = error instanceof Error ? error.message : String(error);
    } finally {
      preparing = false;
    }
  }

  function submit() {
    if (!canSend) return;
    const message = text.trim();
    const attachments = images;
    const steer = running && sendMode === "steer";
    if (pdfs.length) {
      void submitPdf(message, attachments, steer);
      return;
    }
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

  /** Adds images and PDFs from the picker, clipboard, or a drop. */
  async function attach(files: Iterable<File>) {
    if (blocked) return;
    pdfError = "";
    for (const file of files) {
      if (isPdf(file)) {
        if (file.size > 32 * 1024 * 1024 || pdfs.length >= 4) {
          pdfError = "Attach up to four PDFs of 32 MiB each.";
          continue;
        }
        pdfs = [...pdfs, file];
        continue;
      }
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
    if (!files.some((f) => isPdf(f) || f.type.startsWith("image/"))) return;
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
          <button class="remove" disabled={blocked} onclick={() => (images = images.filter((_, j) => j !== i))} aria-label={`Remove image ${i + 1}`} title="Remove"><Icon name="x" size={12} /></button>
        </div>
      {/each}
    </div>
  {/if}
  {#if pdfs.length}
    <div class="thumbs">
      {#each pdfs as pdf, i}
        <span class="document">
          <Icon name="file-text" size={15} />
          <span class="ellipsis" title={pdf.name}>{pdf.name}</span>
          <button class="iconbtn" type="button" disabled={blocked} aria-label={`Remove PDF ${pdf.name}`} onclick={() => { pdfs = pdfs.filter((_, j) => j !== i); pdfError = ""; }}><Icon name="x" size={13} /></button>
        </span>
      {/each}
    </div>
  {/if}
  {#if preparing}<div class="muted small" role="status">Preparing PDF…</div>{/if}
  {#if pdfError}<div class="error small" role="alert">{pdfError}</div>{/if}
  {#if suggestCompact && !blocked}
    <div class="composer-commands">
      <button class="plain" type="button" onclick={chooseCompact}><code>/compact</code> <span class="muted">[instructions]</span></button>
    </div>
  {/if}
  <div class="inputrow">
    <button class="plain attach" onclick={() => picker.click()} disabled={blocked} title="Attach images or PDFs" aria-label="Attach images or PDFs"><Icon name="paperclip" size={16} /></button>
    <input bind:this={picker} type="file" accept="image/*,.pdf,application/pdf" multiple hidden onchange={(e) => { void attach(Array.from(e.currentTarget.files ?? [])); e.currentTarget.value = ""; }} />
    <textarea
      bind:this={input}
      bind:value={text}
      {onkeydown}
      {onpaste}
      rows="1"
      disabled={blocked}
      placeholder={running ? "Message Hibi…" : "Describe a task or ask a question"}
    ></textarea>
    {#if running}
      <button class="send" onclick={onabort} title="Stop (Esc)" aria-label="Stop"><Icon name="square" size={14} /></button>
      {#key blocked}
        <div class="send-split">
          <button class="primary send" onclick={submit} disabled={!canSend} title={sendMode === "steer" ? "Steer current run (Enter)" : "Queue for next turn (Enter)"}>{sendLabel}</button>
          <Menu
            options={[...sendModes]}
            value={sendMode}
            label="Send mode"
            onpick={(mode) => { sendMode = mode; input.focus(); }}
            disabled={blocked}
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
  .document { display: inline-flex; align-items: center; gap: 6px; min-width: 0; max-width: 100%; padding: 2px 6px; border: 1px solid var(--line); border-radius: 6px; font-size: 12px; }
  .document :global(svg) { flex: none; }
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
