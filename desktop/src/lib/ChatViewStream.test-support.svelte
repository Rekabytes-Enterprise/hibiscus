<script lang="ts">
  import ChatView from "./ChatView.svelte";
  import type { Chat } from "./chat";
  import type { ComponentProps } from "svelte";

  let { chat, ...props }: ComponentProps<typeof ChatView> = $props();
  let streamed = $state<Chat>();
  export function show(next: Chat) {
    streamed = next;
  }
</script>

<ChatView {...props} chat={streamed ?? chat} />
