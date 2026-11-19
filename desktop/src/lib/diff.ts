// Parses `git diff` output for the diff pane. Pure, so it is unit-tested.

export type Line = { kind: "add" | "del" | "ctx"; old: number | null; new: number | null; text: string };
export type Hunk = { oldStart: number; newStart: number; lines: Line[] };
export type FileDiff = {
  path: string;
  status: "added" | "deleted" | "renamed" | "modified";
  binary: boolean;
  added: number;
  removed: number;
  hunks: Hunk[];
};

/** `a/x` or `"a/x y"` from a ---/+++ line; null for /dev/null. */
function side(raw: string): string | null {
  let path = raw.replace(/\t$/, "");
  if (path === "/dev/null") return null;
  if (path.startsWith('"')) {
    try {
      path = JSON.parse(path);
    } catch {
      path = path.slice(1, -1);
    }
  }
  return path.replace(/^[ab]\//, "");
}

export function parseDiff(text: string): FileDiff[] {
  const files: FileDiff[] = [];
  let file: FileDiff | null = null;
  let hunk: Hunk | null = null;
  // Lines left in the current hunk; a "--- x" line inside a hunk is content, not a header.
  let oldLeft = 0;
  let newLeft = 0;
  let oldNo = 0;
  let newNo = 0;

  for (const line of text.split("\n")) {
    if (hunk && file && (oldLeft > 0 || newLeft > 0)) {
      const mark = line[0];
      const body = line.slice(1);
      if (mark === "+") {
        hunk.lines.push({ kind: "add", old: null, new: newNo++, text: body });
        file.added++;
        newLeft--;
        continue;
      }
      if (mark === "-") {
        hunk.lines.push({ kind: "del", old: oldNo++, new: null, text: body });
        file.removed++;
        oldLeft--;
        continue;
      }
      if (mark === " " || line === "") {
        hunk.lines.push({ kind: "ctx", old: oldNo++, new: newNo++, text: body });
        oldLeft--;
        newLeft--;
        continue;
      }
      if (mark === "\\") continue; // "\ No newline at end of file"
    }
    if (line.startsWith("diff --git ")) {
      const m = /^diff --git "?a\/(.+?)"? "?b\/(.+?)"?$/.exec(line);
      file = { path: m?.[2] ?? line.slice(11), status: "modified", binary: false, added: 0, removed: 0, hunks: [] };
      files.push(file);
      hunk = null;
      continue;
    }
    if (!file) continue;
    if (line.startsWith("@@")) {
      const m = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/.exec(line);
      if (!m) continue;
      oldNo = +m[1];
      newNo = +m[3];
      oldLeft = m[2] === undefined ? 1 : +m[2];
      newLeft = m[4] === undefined ? 1 : +m[4];
      hunk = { oldStart: oldNo, newStart: newNo, lines: [] };
      file.hunks.push(hunk);
    } else if (line.startsWith("--- ")) {
      if (side(line.slice(4)) === null) file.status = "added";
    } else if (line.startsWith("+++ ")) {
      const path = side(line.slice(4));
      if (path === null) file.status = "deleted";
      else file.path = path;
    } else if (line.startsWith("new file mode")) file.status = "added";
    else if (line.startsWith("deleted file mode")) file.status = "deleted";
    else if (line.startsWith("rename to ")) {
      file.status = "renamed";
      file.path = line.slice(10);
    } else if (line.startsWith("Binary files ")) file.binary = true;
  }
  return files;
}

/** Unchanged lines hidden between two hunks (or before the first). */
export function gapBefore(hunks: Hunk[], index: number): number {
  const hunk = hunks[index];
  if (index === 0) return hunk.oldStart > 1 ? hunk.oldStart - 1 : 0;
  const prev = hunks[index - 1];
  const prevOldEnd = prev.oldStart + prev.lines.filter((l) => l.kind !== "add").length;
  return Math.max(0, hunk.oldStart - prevOldEnd);
}
