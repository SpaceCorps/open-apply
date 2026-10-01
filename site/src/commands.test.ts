import { describe, expect, it } from "vite-plus/test";
import reference from "./generated/commands.json";
import { filterCommands, groupCommands, type CliReference } from "./commands.ts";

const data: CliReference = reference;

describe("generated command reference", () => {
  it("covers every leaf command of the CLI", () => {
    const paths = data.commands.map((c) => c.path);
    for (const expected of [
      "init",
      "doctor",
      "profile show",
      "profile path",
      "profile set",
      "source add",
      "source list",
      "source remove",
      "search",
      "job add",
      "job list",
      "job show",
      "job update",
      "job rm",
      "next",
      "prepare",
      "materials attach",
      "applied",
      "status",
      "event add",
      "triage",
      "stale",
      "followups",
      "stats",
      "export",
      "import",
      "agent-readme",
      "skill install",
    ]) {
      expect(paths, `missing ${expected}`).toContain(expected);
    }
    expect(paths).toHaveLength(28);
    expect(new Set(paths).size).toBe(paths.length);
  });

  it("describes every command and starts usage with the binary name", () => {
    for (const command of data.commands) {
      expect(command.about.length, command.path).toBeGreaterThan(10);
      expect(command.usage.startsWith(`open-apply ${command.path}`), command.path).toBe(true);
      for (const option of command.options) {
        expect(option.flag.startsWith("-"), `${command.path} ${option.flag}`).toBe(true);
      }
    }
  });

  it("documents the global flags once instead of on every command", () => {
    expect(data.global.map((g) => g.flag.split(" ")[0])).toEqual(["--json", "--quiet", "--home"]);
    for (const command of data.commands) {
      expect(
        command.options.some((o) => o.flag.startsWith("--json")),
        command.path,
      ).toBe(false);
    }
  });

  it("keeps the options that make the safety stance real", () => {
    const applied = data.commands.find((c) => c.path === "applied");
    expect(applied?.options.map((o) => o.flag)).toContain("--force");
    const add = data.commands.find((c) => c.path === "job add");
    expect(add?.options.map((o) => o.flag)).toContain("--no-fetch");
    const triage = data.commands.find((c) => c.path === "triage");
    expect(triage?.options.map((o) => o.flag.split(" ")[0])).toContain("--apply");
  });
});

describe("groupCommands", () => {
  it("places every command in exactly one named section", () => {
    const sections = groupCommands(data);
    expect(sections.map((s) => s.title)).toEqual([
      "Setup",
      "Discovery",
      "Jobs and materials",
      "Record and track",
      "Data and agents",
    ]);
    const total = sections.reduce((sum, s) => sum + s.commands.length, 0);
    expect(total).toBe(data.commands.length);
  });

  it("puts unknown commands in Other rather than dropping them", () => {
    const extra = {
      ...data,
      commands: [
        ...data.commands,
        {
          path: "future",
          group: "future",
          about: "A command from the future",
          usage: "open-apply future",
          arguments: [],
          options: [],
        },
      ],
    };
    const sections = groupCommands(extra);
    expect(sections.at(-1)?.title).toBe("Other");
    expect(sections.at(-1)?.commands.map((c) => c.path)).toEqual(["future"]);
  });
});

describe("filterCommands", () => {
  it("returns everything for an empty query", () => {
    expect(filterCommands(data.commands, "  ")).toHaveLength(data.commands.length);
  });

  it("requires every term, case-insensitively, across path, text and flags", () => {
    const hits = filterCommands(data.commands, "GUARDRAILS force").map((c) => c.path);
    expect(hits).toContain("applied");
    expect(hits).not.toContain("init");
    expect(filterCommands(data.commands, "zzzz-no-such-thing")).toEqual([]);
  });
});
