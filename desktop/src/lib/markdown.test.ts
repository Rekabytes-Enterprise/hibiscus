// @vitest-environment jsdom
import { expect, it } from "vitest";
import { cachedMarkdown, markdown } from "./markdown";

it("renders common Markdown", () => {
  const html = markdown("# Title\n\n- **bold** and `code`\n\n```rust\nfn main() {}\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |");
  expect(html).toContain("<h1>Title</h1>");
  expect(html).toContain("<strong>bold</strong>");
  expect(html).toContain("<code>code</code>");
  expect(html).toContain('<code class="hljs language-rust"><span class="hljs-keyword">fn</span> <span class="hljs-title function_">main</span>');
  expect(html).toContain('<div class="code-lang">rust</div>');
  expect(html).toContain('aria-label="Copy code"');
  expect(html).toContain("<table>");
});

it("strips anything that could run or phone home", () => {
  const html = markdown(
    '<script>alert(1)</script><img src=x onerror="alert(2)"><a href="javascript:alert(3)">x</a><style>*{}</style><form><input></form>',
  );
  expect(html).not.toMatch(/<script|onerror|javascript:|<style|<form|<input/);
});

it("escapes code in unknown or missing languages", () => {
  expect(markdown("```nope\n<b>x</b>\n```")).toContain('<code class="hljs">&lt;b&gt;x&lt;/b&gt;');
  expect(markdown("```\na < b\n```")).toContain("a &lt; b");
});

it("remembers rendered text with the same result", () => {
  const text = "**cached** `once`";
  expect(cachedMarkdown(text)).toBe(markdown(text));
  expect(cachedMarkdown(text)).toBe(markdown(text));
});
