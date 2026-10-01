export type Theme = "system" | "light" | "dark";

export const STORAGE_KEY = "open-apply-theme";

const ORDER: readonly Theme[] = ["system", "dark", "light"];

/** system -> dark -> light -> system. */
export function nextTheme(current: Theme): Theme {
  const index = ORDER.indexOf(current);
  return ORDER[(index + 1) % ORDER.length] ?? "system";
}

/** Anything that is not a known explicit choice means "follow the operating system". */
export function parseTheme(value: string | null | undefined): Theme {
  return value === "light" || value === "dark" ? value : "system";
}

export function themeLabel(theme: Theme): string {
  return theme === "system" ? "Theme: system" : `Theme: ${theme}`;
}

export function applyTheme(theme: Theme, root: HTMLElement): void {
  if (theme === "system") {
    delete root.dataset.theme;
  } else {
    root.dataset.theme = theme;
  }
}
