import { describe, expect, it } from "vite-plus/test";
import { applyTheme, nextTheme, parseTheme, themeLabel } from "./theme.ts";

describe("theme", () => {
  it("cycles system, dark, light and back", () => {
    expect(nextTheme("system")).toBe("dark");
    expect(nextTheme("dark")).toBe("light");
    expect(nextTheme("light")).toBe("system");
  });

  it("treats anything but an explicit choice as the operating system setting", () => {
    expect(parseTheme("dark")).toBe("dark");
    expect(parseTheme("light")).toBe("light");
    expect(parseTheme("blue")).toBe("system");
    expect(parseTheme(null)).toBe("system");
    expect(parseTheme(undefined)).toBe("system");
  });

  it("labels the button", () => {
    expect(themeLabel("system")).toBe("Theme: system");
    expect(themeLabel("dark")).toBe("Theme: dark");
  });

  it("sets and clears the data attribute", () => {
    const root = { dataset: {} as Record<string, string | undefined> } as unknown as HTMLElement;
    applyTheme("dark", root);
    expect(root.dataset.theme).toBe("dark");
    applyTheme("system", root);
    expect(root.dataset.theme).toBeUndefined();
  });
});
