// @vitest-environment jsdom
import { mount, unmount } from "svelte";
import { expect, it, vi } from "vitest";
import { apply, empty } from "./chat";
import { rekaUsage } from "./engine";
import { openUrl } from "@tauri-apps/plugin-opener";
import RooxcodeUsageNotice from "./RooxcodeUsageNotice.svelte";

vi.mock("./engine", () => ({ rekaUsage: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn(async () => {}) }));

it("shows a weekly limit and opens the fixed Rooxcode website", async () => {
  vi.mocked(rekaUsage).mockResolvedValue({
    account: { name: null, email: null, avatar: null },
    fiveHours: { percent: 18, dropsAt: null, retryAfter: null },
    week: { percent: 100, dropsAt: null, retryAfter: 79354 },
    credits: 0,
    device: null,
  });
  const target = document.body.appendChild(document.createElement("div"));
  const ondismiss = vi.fn();
  const view = mount(RooxcodeUsageNotice, { target, props: { retryAfter: 79354, ondismiss } });
  try {
    await vi.waitFor(() => expect(target.textContent).toContain("Weekly usage limit reached"));
    expect(target.textContent).toContain("Room again");
    target.querySelector<HTMLButtonElement>(".more-usage")?.click();
    expect(openUrl).toHaveBeenCalledWith("https://rooxcode.reka-bytes.my");
    target.querySelector<HTMLButtonElement>('[aria-label="Dismiss usage notice"]')?.click();
    expect(ondismiss).toHaveBeenCalledOnce();
  } finally {
    await unmount(view);
    target.remove();
  }
});

it("keeps rate and capacity failures separate from account usage", () => {
  const failure = { role: "assistant", stopReason: "error", errorMessage: "Rooxcode HTTP 429: retry later" };
  const capacity = apply(empty(), { type: "message_end", message: failure });
  expect(capacity.rooxcodeAccountLimit).toBeNull();
  expect(capacity.items.at(-1)).toMatchObject({ kind: "notice", error: true });
  const account = apply(empty(), { type: "message_end", message: {
    ...failure, rooxcodeAccountLimit: { retryAfter: 79354 },
  } });
  expect(account.rooxcodeAccountLimit).toEqual({ retryAfter: 79354 });
  expect(account.items).toEqual([]);
  expect(apply(account, { type: "agent_settled" }).rooxcodeAccountLimit).toEqual({ retryAfter: 79354 });
  expect(apply(account, { type: "agent_start" }).rooxcodeAccountLimit).toBeNull();
});
