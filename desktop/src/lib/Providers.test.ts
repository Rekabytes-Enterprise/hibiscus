// @vitest-environment jsdom
import { mount, tick, unmount } from "svelte";
import { expect, it, vi } from "vitest";
import Providers from "./Providers.svelte";

const desktop = vi.hoisted(() => ({
  detectLocalModels: vi.fn(),
  addLocalProvider: vi.fn(),
  listProviders: vi.fn().mockResolvedValue([]),
  rekaAccount: vi.fn().mockResolvedValue(null),
  onCodexLogin: vi.fn().mockResolvedValue(() => {}),
}));
vi.mock("./engine", () => desktop);
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

it("detects the entered server, previews its models and saves discovery mode", async () => {
  desktop.detectLocalModels.mockResolvedValue([
    { id: "fixture-model", contextWindow: 32768, reasoning: true, images: false },
  ]);
  desktop.addLocalProvider.mockResolvedValue(undefined);
  const host = document.createElement("div");
  document.body.append(host);
  const component = mount(Providers, {
    target: host,
    props: { onsaved: async () => {}, onsignin: () => {} },
  });

  function click(name: string) {
    const button = [...host.querySelectorAll("button")].find((b) => b.textContent?.trim() === name);
    if (!button) throw new Error(`Missing button ${name}`);
    button.click();
  }

  function fill(name: string, value: string) {
    const label = [...host.querySelectorAll("label")].find((l) => l.textContent?.trim() === name);
    const input = label?.querySelector("input");
    if (!input) throw new Error(`Missing field ${name}`);
    input.value = value;
    input.dispatchEvent(new Event("input", { bubbles: true }));
  }

  try {
    click("Add local AI");
    await tick();
    expect(host.querySelector('input[placeholder="http://localhost:8000/v1"]')?.getAttribute("value")).toBeNull();
    fill("Name", "ornith");
    fill("Server URL", "http://fixture.local:9000");
    await tick();
    click("Detect models");
    await vi.waitFor(() => expect(host.textContent).toContain("fixture-model"));
    expect(desktop.detectLocalModels).toHaveBeenCalledWith("http://fixture.local:9000", "");
    expect(host.textContent).toContain("Reasoning reported");

    fill("Server URL", "http://other.local:9000");
    await tick();
    expect(host.textContent).not.toContain("fixture-model");
    click("Detect models");
    await vi.waitFor(() => expect(host.textContent).toContain("fixture-model"));
    click("Add");
    await vi.waitFor(() => expect(desktop.addLocalProvider).toHaveBeenCalledWith({
      id: "ornith", baseUrl: "http://other.local:9000", api: "chat_completions",
      models: [], contextWindow: null, discoverModels: true, key: "",
    }));
  } finally {
    await unmount(component);
    host.remove();
  }
});
