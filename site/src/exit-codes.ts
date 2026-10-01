export interface ExitCode {
  code: number;
  name: string;
  meaning: string;
}

/** Mirrors the table in the README and `open-apply agent-readme --json`. */
export const EXIT_CODES: readonly ExitCode[] = [
  { code: 0, name: "ok", meaning: "Command completed successfully." },
  { code: 1, name: "internal", meaning: "Unexpected failure. Report it and stop." },
  { code: 2, name: "usage", meaning: "Bad flags or arguments. The hint has the usage line." },
  {
    code: 3,
    name: "not_found",
    meaning: "Unknown job id, uninitialized home, or no matching application.",
  },
  {
    code: 4,
    name: "validation",
    meaning: "Input rejected (status, date, URL, profile key). Also doctor with a failing check.",
  },
  {
    code: 5,
    name: "network",
    meaning:
      "Upstream unreachable, refusing, or returning something unexpected. Never worked around.",
  },
  {
    code: 6,
    name: "guardrail",
    meaning: "Daily cap, company cooldown or missing materials. --force overrides, on purpose.",
  },
  {
    code: 7,
    name: "conflict",
    meaning: "Duplicate job, application already recorded, or an ambiguous email match.",
  },
];
