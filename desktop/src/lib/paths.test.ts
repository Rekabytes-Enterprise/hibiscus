import { expect, it } from "vitest";
import { name, where, wslDistro } from "./paths";

it("labels folders relative to home", () => {
  expect(name("/Users/me/Desktop/Github/")).toBe("Github");
  expect(where("/Users/me/Desktop/Github", "/Users/me")).toBe("~/Desktop");
  expect(where("/Users/me/project", "/Users/me/")).toBe("~");
  expect(where("/Users/meow/project", "/Users/me")).toBe("/Users/meow");
  expect(where("/opt/x", "")).toBe("/opt");
});

it("labels Windows folders relative to home", () => {
  expect(name("C:\\Users\\me\\Desktop\\Github\\")).toBe("Github");
  expect(where("C:\\Users\\me\\Desktop\\Github", "C:\\Users\\me")).toBe("~\\Desktop");
  expect(where("C:\\Users\\me\\project", "C:\\Users\\me\\")).toBe("~");
  expect(where("D:\\work\\x", "C:\\Users\\me")).toBe("D:\\work");
});

it("finds the WSL distro a folder lives in", () => {
  expect(wslDistro("\\\\wsl.localhost\\Ubuntu\\home\\me")).toBe("Ubuntu");
  expect(wslDistro("\\\\wsl$\\Debian")).toBe("Debian");
  expect(wslDistro("\\\\?\\UNC\\wsl.localhost\\Ubuntu\\x")).toBe("Ubuntu");
  expect(wslDistro("//wsl.localhost/Ubuntu/home")).toBe("Ubuntu");
  expect(wslDistro("C:\\Users\\me")).toBeUndefined();
  expect(wslDistro("/home/me")).toBeUndefined();
});
