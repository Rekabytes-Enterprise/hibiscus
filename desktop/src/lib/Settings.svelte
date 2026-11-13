<script lang="ts">
  import About from "./About.svelte";
  import Account from "./Account.svelte";
  import type { RekaAccount } from "./engine";
  import Icon from "./Icon.svelte";
  import { TERMS } from "./legal";
  import { markdown } from "./markdown";
  import Privacy from "./Privacy.svelte";
  import Providers from "./Providers.svelte";

  type Page = "account" | "providers" | "privacy" | "terms" | "about";
  let {
    onback,
    onsaved,
    onsignin,
    onsignout,
    account,
    onaccount,
    start = "providers",
  }: {
    onback: () => void;
    onsaved: () => Promise<void>;
    onsignin: () => void;
    onsignout: () => void;
    account: RekaAccount | null;
    /** The fresh profile from the Account page. */
    onaccount: (account: RekaAccount) => void;
    /** The page to open on, such as "account" from the sidebar's Manage account. */
    start?: Page;
  } = $props();
  // svelte-ignore state_referenced_locally
  let page = $state<Page>(start);
</script>

<div class="settings">
  <nav>
    <button class="nav" onclick={onback}><Icon name="arrow-left" /> Back</button>
    <div class="heading muted small">Settings</div>
    <button class="nav" class:active={page === "account"} onclick={() => (page = "account")}><Icon name="user" /> Account</button>
    <button class="nav" class:active={page === "providers"} onclick={() => (page = "providers")}><Icon name="key" /> Providers</button>
    <button class="nav" class:active={page === "privacy"} onclick={() => (page = "privacy")}><Icon name="shield" /> Privacy</button>
    <button class="nav" class:active={page === "terms"} onclick={() => (page = "terms")}><Icon name="file-text" /> Terms of Use</button>
    <button class="nav" class:active={page === "about"} onclick={() => (page = "about")}><Icon name="info" /> About</button>
  </nav>
  <section>
    {#if page === "account"}
      <Account {account} {onsignin} {onsignout} onloaded={onaccount} />
    {:else if page === "providers"}
      <Providers {onsaved} {onsignin} />
    {:else if page === "privacy"}
      <Privacy />
    {:else if page === "about"}
      <About />
    {:else}
      <h2>Terms of Use</h2>
      <div class="md legal">{@html markdown(TERMS)}</div>
    {/if}
  </section>
</div>
