import { expect, it } from "vitest";
import { name, where } from "./paths";

it("labels folders relative to home", () => {
  expect(name("/Users/me/Desktop/Github/")).toBe("Github");
  expect(where("/Users/me/Desktop/Github", "/Users/me")).toBe("~/Desktop");
  expect(where("/Users/me/project", "/Users/me/")).toBe("~");
  expect(where("/Users/meow/project", "/Users/me")).toBe("/Users/meow");
  expect(where("/opt/x", "")).toBe("/opt");
});
