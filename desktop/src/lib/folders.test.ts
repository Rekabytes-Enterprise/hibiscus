import { expect, it } from "vitest";
import { added, LIMIT, opened, removed } from "./folders";

it("moves an opened folder to the front and keeps the limit", () => {
  const many = Array.from({ length: LIMIT }, (_, i) => `/f${i}`);
  expect(opened(["/a", "/b"], "/b")).toEqual(["/b", "/a"]);
  expect(opened(many, "/new")).toEqual(["/new", ...many.slice(0, LIMIT - 1)]);
});

it("adds a folder behind the open one without reordering known folders", () => {
  expect(added([], "/a")).toEqual(["/a"]);
  expect(added(["/open", "/b"], "/new")).toEqual(["/open", "/new", "/b"]);
  expect(added(["/open", "/b"], "/b")).toEqual(["/open", "/b"]);
});

it("removes a folder and nothing else", () => {
  expect(removed(["/a", "/b", "/c"], "/b")).toEqual(["/a", "/c"]);
  expect(removed(["/a"], "/missing")).toEqual(["/a"]);
});
