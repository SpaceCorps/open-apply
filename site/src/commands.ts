export interface CliOption {
  flag: string;
  description: string;
}

export interface CliArgument {
  name: string;
  description: string;
}

export interface CliCommand {
  /** Space-separated path such as `job add`. */
  path: string;
  /** The top-level command it belongs to. */
  group: string;
  about: string;
  usage: string;
  arguments: CliArgument[];
  options: CliOption[];
}

export interface CliReference {
  version: string;
  about: string;
  global: CliOption[];
  commands: CliCommand[];
}

export interface CommandSection {
  title: string;
  commands: CliCommand[];
}

const SECTIONS: readonly { title: string; groups: readonly string[] }[] = [
  { title: "Setup", groups: ["init", "doctor", "profile"] },
  { title: "Discovery", groups: ["source", "search"] },
  { title: "Jobs and materials", groups: ["job", "next", "prepare", "materials"] },
  {
    title: "Record and track",
    groups: ["applied", "status", "event", "triage", "stale", "followups", "stats"],
  },
  { title: "Data and agents", groups: ["export", "import", "agent-readme", "skill"] },
];

/** Puts every command in a section; anything new that is not listed above lands in "Other". */
export function groupCommands(reference: CliReference): CommandSection[] {
  const sections: CommandSection[] = SECTIONS.map((s) => ({ title: s.title, commands: [] }));
  const other: CommandSection = { title: "Other", commands: [] };
  for (const command of reference.commands) {
    const index = SECTIONS.findIndex((s) => s.groups.includes(command.group));
    (index >= 0 ? sections[index] : other).commands.push(command);
  }
  if (other.commands.length > 0) sections.push(other);
  return sections.filter((s) => s.commands.length > 0);
}

function searchText(command: CliCommand): string {
  const parts = [command.path, command.about, command.usage];
  for (const o of command.options) parts.push(o.flag, o.description);
  for (const a of command.arguments) parts.push(a.name, a.description);
  return parts.join(" ").toLowerCase();
}

/** Every whitespace-separated term in `query` must appear somewhere in the command's text. */
export function filterCommands(commands: readonly CliCommand[], query: string): CliCommand[] {
  const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (terms.length === 0) return [...commands];
  return commands.filter((c) => {
    const text = searchText(c);
    return terms.every((t) => text.includes(t));
  });
}
