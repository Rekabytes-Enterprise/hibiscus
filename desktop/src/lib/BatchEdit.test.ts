// @vitest-environment jsdom
import { mount, tick, unmount } from "svelte";
import { expect, it, vi } from "vitest";
import ToolCall from "./ToolCall.svelte";
import { apply, empty, outputs } from "./chat";

vi.mock("./engine", () => ({ request: vi.fn() }));

it("shows batch changes in one tool row with separate file diffs", async () => {
  const start = apply(empty(), {
    type: "tool_execution_start", toolName: "batch_edit", toolCallId: "batch",
    args: { edits: [{ path: "one.txt" }, { path: "one.txt" }, { path: "two.txt" }] },
  });
  const chat = apply(start, {
    type: "tool_execution_end", toolName: "batch_edit", toolCallId: "batch",
    result: { content: [{ type: "text", text: "Edited 2 files." }], details: {
      editedPaths: ["one.txt", "two.txt"],
      diff: 'diff --git "a/one.txt" "b/one.txt"\n--- "a/one.txt"\n+++ "b/one.txt"\n@@ -1,1 +1,1 @@\n-old first\n+new first\ndiff --git "a/two.txt" "b/two.txt"\n--- "a/two.txt"\n+++ "b/two.txt"\n@@ -1,1 +1,1 @@\n-old second\n+new second\n',
    } },
  });
  expect(outputs(chat)).toEqual(["one.txt", "two.txt"]);
  const [tool] = chat.items;
  if (tool?.kind !== "tool") throw new Error("Missing batch edit tool row");

  const target = document.body.appendChild(document.createElement("div"));
  const view = mount(ToolCall, { target, props: { tool, folder: null } });
  try {
    const row = target.querySelector<HTMLButtonElement>("button.toolrow");
    expect(row?.textContent).toContain("2 files");
    row?.click();
    await tick();
    expect(target.querySelectorAll(".toolrow")).toHaveLength(1);
    expect(target.textContent).toContain("one.txt");
    expect(target.textContent).toContain("two.txt");
    expect(target.textContent).toContain("new first");
    expect(target.textContent).toContain("new second");
  } finally {
    await unmount(view);
    target.remove();
  }
});
