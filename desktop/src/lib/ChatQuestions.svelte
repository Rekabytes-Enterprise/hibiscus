<script lang="ts">
  import type { Question } from "./chat";

  let { questions, disabled, onanswer }: {
    questions: Question[];
    disabled: boolean;
    onanswer: (text: string) => Promise<void>;
  } = $props();

  let answers = $state<string[]>([]);
  let sending = $state(false);
  const complete = $derived(questions.every((_, i) => answers[i]?.trim()));

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!complete || disabled || sending) return;
    sending = true;
    try {
      await onanswer(questions.map(({ question }, i) => `- ${question}\n  ${answers[i].trim()}`).join("\n"));
    } finally {
      sending = false;
    }
  }
</script>

<form onsubmit={submit} aria-label="Questions from Hibi">
  {#each questions as { question, options }, i (i)}
    <fieldset disabled={disabled || sending}>
      <legend>{question}</legend>
      <div class="options">
        {#each options as option}
          <button type="button" aria-pressed={answers[i] === option} onclick={() => (answers[i] = option)}>
            {option}
          </button>
        {/each}
      </div>
      <input aria-label={question} placeholder="Or type an answer" bind:value={answers[i]} />
    </fieldset>
  {/each}
  <button class="submit" type="submit" disabled={!complete || disabled || sending}>Send answers</button>
</form>

<style>
  form { display: grid; gap: 12px; padding: 14px; border: 1px solid var(--line); border-radius: 12px; background: var(--user); }
  fieldset { display: grid; gap: 8px; min-width: 0; border: 0; margin: 0; padding: 0; }
  legend { font-weight: 600; margin-bottom: 6px; }
  .options { display: flex; flex-wrap: wrap; gap: 6px; }
  button, input { padding: 6px 10px; border: 1px solid var(--line); border-radius: 6px; }
  input { width: 100%; }
  button[aria-pressed="true"], .submit { border-color: var(--accent); }
  button:disabled { opacity: .55; }
  .submit { justify-self: end; }
</style>
