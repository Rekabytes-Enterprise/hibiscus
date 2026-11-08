import { describe, expect, it } from "vitest";
import { gapBefore, parseDiff } from "./diff";

const sample = `diff --git a/src/a.ts b/src/a.ts
index 1111111..2222222 100644
--- a/src/a.ts
+++ b/src/a.ts
@@ -1,3 +1,3 @@
 one
-two
+TWO
 three
@@ -10,2 +10,3 @@ function f() {
 ten
--- not a header, a removed line starting with dashes
+++ not a header, an added line
+more
\\ No newline at end of file
diff --git a/old.txt b/old.txt
deleted file mode 100644
--- a/old.txt
+++ /dev/null
@@ -1 +0,0 @@
-bye
diff --git a/new file.txt b/new file.txt
new file mode 100644
--- /dev/null
+++ b/new file.txt
@@ -0,0 +1 @@
+hello
diff --git a/logo.png b/logo.png
Binary files a/logo.png and b/logo.png differ
diff --git a/x.ts b/y.ts
similarity 90%
rename from x.ts
rename to y.ts
`;

describe("parseDiff", () => {
  const files = parseDiff(sample);

  it("finds every file with its status", () => {
    expect(files.map((f) => [f.path, f.status, f.binary])).toEqual([
      ["src/a.ts", "modified", false],
      ["old.txt", "deleted", false],
      ["new file.txt", "added", false],
      ["logo.png", "modified", true],
      ["y.ts", "renamed", false],
    ]);
  });

  it("counts and numbers lines, treating dashes inside a hunk as content", () => {
    const a = files[0];
    expect([a.added, a.removed]).toEqual([3, 2]);
    expect(a.hunks[0].lines).toEqual([
      { kind: "ctx", old: 1, new: 1, text: "one" },
      { kind: "del", old: 2, new: null, text: "two" },
      { kind: "add", old: null, new: 2, text: "TWO" },
      { kind: "ctx", old: 3, new: 3, text: "three" },
    ]);
    expect(a.hunks[1].lines.map((l) => [l.kind, l.old, l.new])).toEqual([
      ["ctx", 10, 10],
      ["del", 11, null],
      ["add", null, 11],
      ["add", null, 12],
    ]);
  });

  it("measures the unchanged gaps between hunks", () => {
    const a = files[0];
    expect(gapBefore(a.hunks, 0)).toBe(0);
    expect(gapBefore(a.hunks, 1)).toBe(6); // lines 4..9
  });

  it("handles empty input", () => {
    expect(parseDiff("")).toEqual([]);
  });
});
