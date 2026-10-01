import "./style.css";
import reference from "./generated/commands.json";
import { groupCommands, filterCommands, type CliCommand, type CliReference } from "./commands.ts";
import { EXIT_CODES } from "./exit-codes.ts";
import { STORAGE_KEY, applyTheme, nextTheme, parseTheme, themeLabel, type Theme } from "./theme.ts";

const data: CliReference = reference;

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  options: { text?: string; className?: string } = {},
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (options.text !== undefined) node.textContent = options.text;
  if (options.className !== undefined) node.className = options.className;
  return node;
}

function renderGlobalFlags(): void {
  const target = document.getElementById("global-flags");
  if (!target) return;
  data.global.forEach((flag, i) => {
    if (i > 0) target.append(", ");
    target.append(el("code", { text: flag.flag }));
  });
}

function renderCommand(command: CliCommand): HTMLElement {
  const details = el("details", { className: "command" });
  details.id = `cmd-${command.path.replace(/\s+/g, "-")}`;
  const summary = el("summary");
  summary.append(el("code", { text: `open-apply ${command.path}` }));
  summary.append(el("span", { text: command.about, className: "about" }));
  details.append(summary);

  const body = el("div", { className: "command-body" });
  body.append(el("pre", { text: command.usage }));
  const rows = [
    ...command.arguments.map((a) => ({ name: a.name, description: a.description })),
    ...command.options.map((o) => ({ name: o.flag, description: o.description })),
  ];
  if (rows.length > 0) {
    const list = el("dl");
    for (const row of rows) {
      list.append(el("dt", { text: row.name }));
      list.append(el("dd", { text: row.description || "" }));
    }
    body.append(list);
  }
  details.append(body);
  return details;
}

function renderCommands(query: string): void {
  const target = document.getElementById("command-list");
  if (!target) return;
  target.replaceChildren();
  const filtered: CliReference = { ...data, commands: filterCommands(data.commands, query) };
  const sections = groupCommands(filtered);
  if (sections.length === 0) {
    target.append(el("p", { text: "No command matches that filter.", className: "muted" }));
    return;
  }
  for (const section of sections) {
    const block = el("section", { className: "command-section" });
    block.append(el("h3", { text: section.title }));
    for (const command of section.commands) block.append(renderCommand(command));
    target.append(block);
  }
}

function renderExitCodes(): void {
  const body = document.querySelector("#exit-table tbody");
  if (!body) return;
  for (const entry of EXIT_CODES) {
    const row = el("tr");
    row.append(el("td", { text: String(entry.code) }));
    const code = el("td");
    code.append(el("code", { text: entry.name }));
    row.append(code);
    row.append(el("td", { text: entry.meaning }));
    body.append(row);
  }
}

function setUpTheme(): void {
  const button = document.getElementById("theme-toggle");
  if (!(button instanceof HTMLButtonElement)) return;
  let theme: Theme = parseTheme(localStorage.getItem(STORAGE_KEY));
  const show = () => {
    button.textContent = themeLabel(theme);
  };
  show();
  button.addEventListener("click", () => {
    theme = nextTheme(theme);
    applyTheme(theme, document.documentElement);
    if (theme === "system") {
      localStorage.removeItem(STORAGE_KEY);
    } else {
      localStorage.setItem(STORAGE_KEY, theme);
    }
    show();
  });
}

function setUpCopyButtons(): void {
  for (const block of document.querySelectorAll<HTMLElement>("[data-copy]")) {
    const code = block.querySelector("code");
    if (!code) continue;
    const button = el("button", { text: "Copy", className: "copy" });
    button.type = "button";
    button.addEventListener("click", () => {
      void navigator.clipboard.writeText(code.textContent ?? "").then(() => {
        button.textContent = "Copied";
        window.setTimeout(() => {
          button.textContent = "Copy";
        }, 1500);
      });
    });
    block.append(button);
  }
}

renderGlobalFlags();
renderCommands("");
renderExitCodes();
setUpTheme();
setUpCopyButtons();

const filter = document.getElementById("command-filter");
if (filter instanceof HTMLInputElement) {
  filter.addEventListener("input", () => {
    renderCommands(filter.value);
  });
}

const version = document.getElementById("version");
if (version) version.textContent = `Command reference generated from open-apply ${data.version}.`;
