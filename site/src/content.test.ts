import { readFileSync } from "node:fs";
import { describe, expect, it } from "vite-plus/test";
import { EXIT_CODES } from "./exit-codes.ts";

const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");
const readme = readFileSync(new URL("../../README.md", import.meta.url), "utf8");
const viteConfig = readFileSync(new URL("../vite.config.ts", import.meta.url), "utf8");

describe("exit codes", () => {
  it("are the eight the CLI defines, in order", () => {
    expect(EXIT_CODES.map((e) => [e.code, e.name])).toEqual([
      [0, "ok"],
      [1, "internal"],
      [2, "usage"],
      [3, "not_found"],
      [4, "validation"],
      [5, "network"],
      [6, "guardrail"],
      [7, "conflict"],
    ]);
  });

  it("match the table in the README", () => {
    for (const entry of EXIT_CODES) {
      expect(readme, `README row for ${entry.name}`).toContain(
        `| \`${entry.code}\` | \`${entry.name}\` |`,
      );
    }
  });
});

describe("landing page copy", () => {
  it("has no emojis, no em-dashes and no other non-ASCII characters", () => {
    for (const [index, line] of html.split("\n").entries()) {
      // eslint-disable-next-line no-control-regex
      expect(/^[\x00-\x7F]*$/.test(line), `index.html:${index + 1}: ${line}`).toBe(true);
    }
  });

  it("carries the install command and the four safety statements", () => {
    expect(html).toContain("cargo install --git https://github.com/SpaceCorps/open-apply --locked");
    for (const claim of [
      "does not scrape LinkedIn",
      "does not bypass CAPTCHAs or bot detection",
      "does not store site passwords",
      "does not submit forms itself",
    ]) {
      expect(html).toContain(claim);
    }
  });

  it("names every section the nav links to", () => {
    for (const id of ["loop", "install", "quickstart", "commands", "exit-codes", "safety"]) {
      expect(html, `#${id}`).toContain(`id="${id}"`);
      expect(html, `nav link to #${id}`).toContain(`href="#${id}"`);
    }
  });

  it("has no placeholder left from the generator", () => {
    expect(html).not.toContain("@@");
  });
});

describe("build configuration", () => {
  it("serves from /open-apply/ for GitHub Pages", () => {
    expect(viteConfig).toContain('base: "/open-apply/"');
  });
});
