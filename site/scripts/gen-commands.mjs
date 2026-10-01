// Builds src/generated/commands.json from the real CLI, so the site's command reference
// is read from `open-apply --help` instead of being typed by hand.
//
//   node scripts/gen-commands.mjs [path-to-open-apply-binary]
//
// Without an argument it uses OPEN_APPLY_BIN, then ../target/release, then ../target/debug.

import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const exe = process.platform === "win32" ? ".exe" : "";

function findBinary() {
  const candidates = [
    process.argv[2],
    process.env.OPEN_APPLY_BIN,
    resolve(here, "../../target/release/open-apply" + exe),
    resolve(here, "../../target/debug/open-apply" + exe),
  ].filter(Boolean);
  const found = candidates.find((c) => existsSync(c));
  if (!found) {
    console.error("open-apply binary not found. Run `cargo build --release` first or pass a path.");
    process.exit(1);
  }
  return found;
}

const bin = findBinary();

function run(args) {
  return execFileSync(bin, args, {
    encoding: "utf8",
    env: { ...process.env, NO_COLOR: "1" },
  }).replace(/\r\n/g, "\n");
}

/** Splits clap's help into its blocks: the about text, the usage line and the named sections. */
function parseHelp(text) {
  const lines = text.split("\n");
  const aboutLines = [];
  let i = 0;
  while (i < lines.length && lines[i].trim() !== "") aboutLines.push(lines[i++].trim());
  const sections = {};
  let usage = "";
  let current = null;
  for (; i < lines.length; i++) {
    const line = lines[i];
    if (line.startsWith("Usage:")) {
      usage = line.slice("Usage:".length).trim();
      current = null;
    } else if (/^[A-Z][A-Za-z]+:$/.test(line)) {
      current = line.slice(0, -1);
      sections[current] = [];
    } else if (current && line.trim() !== "") {
      sections[current].push(line);
    } else if (line.trim() === "") {
      current = null;
    }
  }
  return { about: aboutLines.join(" "), usage, sections };
}

/** `  name   description` rows, with wrapped continuation lines folded into the description. */
function parseRows(rows, splitAt = /\s{2,}/) {
  const out = [];
  for (const row of rows) {
    const indent = row.length - row.trimStart().length;
    const body = row.trim();
    const parts = body.split(splitAt);
    const startsNew =
      indent <= 6 && (body.startsWith("-") || body.startsWith("<") || /^[a-z]/.test(body));
    if (startsNew) {
      const [left, ...rest] = parts;
      out.push({ left, description: rest.join(" ").trim() });
    } else if (out.length > 0) {
      out[out.length - 1].description = `${out[out.length - 1].description} ${body}`.trim();
    }
  }
  return out;
}

const GLOBAL_FLAGS = new Set(["--json", "--quiet", "--home", "-h, --help", "-V, --version"]);
const flagKey = (left) => left.split(/\s/)[0].replace(/,$/, "");

const top = parseHelp(run(["--help"]));
const version = run(["--version"]).trim().split(" ").pop();
const global = parseRows(top.sections.Options ?? [])
  .filter((o) => ["--json", "--quiet", "--home"].includes(flagKey(o.left)))
  .map((o) => ({ flag: o.left, description: o.description }));

const commands = [];
for (const entry of parseRows(top.sections.Commands ?? [])) {
  const name = entry.left;
  if (name === "help") continue;
  const help = parseHelp(run([name, "--help"]));
  const subs = parseRows(help.sections.Commands ?? []).filter((s) => s.left !== "help");
  const targets = subs.length > 0 ? subs.map((s) => [name, s.left]) : [[name]];
  for (const path of targets) {
    const leaf = parseHelp(run([...path, "--help"]));
    const options = parseRows(leaf.sections.Options ?? [])
      .filter((o) => !GLOBAL_FLAGS.has(flagKey(o.left)) && !GLOBAL_FLAGS.has(o.left))
      .map((o) => ({ flag: o.left, description: o.description }));
    const args = parseRows(leaf.sections.Arguments ?? []).map((a) => ({
      name: a.left,
      description: a.description,
    }));
    commands.push({
      path: path.join(" "),
      group: name,
      about: leaf.about,
      usage: leaf.usage,
      arguments: args,
      options,
    });
  }
}

const out = {
  version,
  about: top.about,
  global,
  commands,
};
const file = resolve(here, "../src/generated/commands.json");
mkdirSync(dirname(file), { recursive: true });
writeFileSync(file, JSON.stringify(out, null, 2) + "\n");
console.log(`wrote ${commands.length} commands for open-apply ${version} to ${file}`);
