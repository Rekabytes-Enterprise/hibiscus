import { expect, it } from "vitest";
import { added, moved, opened, removed } from "./folders";

it("keeps a known folder in place when it is opened, and puts a new one last", () => {
  expect(opened(["/a", "/b", "/c"], "/c")).toEqual(["/a", "/b", "/c"]);
  expect(opened(["/a", "/b"], "/new")).toEqual(["/a", "/b", "/new"]);
  const many = Array.from({ length: 12 }, (_, i) => `/f${i}`);
  expect(opened(many, "/new")).toHaveLength(13);
});

it("adds a folder last without reordering known folders", () => {
  expect(added([], "/a")).toEqual(["/a"]);
  expect(added(["/open", "/b"], "/new")).toEqual(["/open", "/b", "/new"]);
  expect(added(["/open", "/b"], "/b")).toEqual(["/open", "/b"]);
});

it("moves a dragged folder into the place of the one it is dropped on", () => {
  expect(moved(["/a", "/b", "/c"], "/c", "/a")).toEqual(["/c", "/a", "/b"]);
  expect(moved(["/a", "/b", "/c"], "/a", "/c")).toEqual(["/b", "/c", "/a"]);
  expect(moved(["/a", "/b", "/c"], "/a", "/b")).toEqual(["/b", "/a", "/c"]);
  expect(moved(["/a", "/b"], "/a", "/missing")).toEqual(["/a", "/b"]);
  expect(moved(["/a", "/b"], "/a", "/a")).toEqual(["/a", "/b"]);
});

it("removes a folder and nothing else", () => {
  expect(removed(["/a", "/b", "/c"], "/b")).toEqual(["/a", "/c"]);
  expect(removed(["/a"], "/missing")).toEqual(["/a"]);
});
