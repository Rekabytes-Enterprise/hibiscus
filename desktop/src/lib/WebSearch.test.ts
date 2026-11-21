// @vitest-environment jsdom
import { mount, tick, unmount } from "svelte";
import { expect, it, vi } from "vitest";
import WebSearch from "./WebSearch.svelte";

const connector = vi.hoisted(() => ({
  status: vi.fn(),
  connect: vi.fn(),
  disconnect: vi.fn(),
}));
vi.mock("./engine", () => ({ webSearch: connector }));

it("preserves an actionable error on bad keys, then connects and disconnects", async () => {
  connector.status.mockResolvedValue(null);
  connector.connect.mockRejectedValueOnce("Brave Search rejected the API key.");
  connector.connect.mockResolvedValue(undefined);
  connector.disconnect.mockResolvedValue(undefined);
  const host = document.createElement("ul");
  document.body.append(host);
  const component = mount(WebSearch, { target: host });
  const click = (label: string) => {
    const button = [...host.querySelectorAll("button")].find((button) => button.textContent?.trim() === label);
    if (!button) throw new Error(`Missing button: ${label}`);
    button.click();
  };
  const enter = async (value: string) => {
    const field = host.querySelector('input[type="password"]');
    if (!(field instanceof HTMLInputElement)) throw new Error("Missing masked key input");
    field.value = value;
    field.dispatchEvent(new Event("input", { bubbles: true }));
    await tick();
  };

  try {
    await vi.waitFor(() => expect(host.textContent).toContain("Not connected"));
    click("Add API key");
    await tick();
    await enter("synthetic-bad");
    click("Connect");
    await vi.waitFor(() => expect(host.textContent).toContain("rejected the API key"));
    expect(connector.connect).toHaveBeenCalledWith("synthetic-bad");
    expect(host.textContent).not.toContain("synthetic-bad");

    await enter("synthetic-good");
    click("Connect");
    await vi.waitFor(() => expect(host.textContent).toContain("Connected"));
    expect(host.querySelector("input")).toBeNull();
    click("Disconnect");
    await vi.waitFor(() => expect(host.textContent).toContain("Not connected"));
    expect(connector.disconnect).toHaveBeenCalledOnce();
  } finally {
    await unmount(component);
    host.remove();
  }
});
