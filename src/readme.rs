//! The manual an agent reads before its first call. It is the same text as
//! `skills/open-apply/SKILL.md`, embedded at build time so the binary is self-describing.

/// The Claude Code skill, including its front matter.
pub const SKILL: &str = include_str!("../skills/open-apply/SKILL.md");

/// Exit codes, as documented in the README and the skill.
pub const EXIT_CODES: &[(u8, &str, &str)] = &[
    (0, "ok", "success"),
    (1, "internal", "unexpected failure: report it and stop"),
    (2, "usage", "bad flags or arguments: fix the call"),
    (3, "not_found", "unknown job id, missing home, or unknown source: do not retry"),
    (4, "validation", "input rejected: read the message and hint, then fix the input"),
    (5, "network", "upstream problem: retry once later, do not work around blocks"),
    (6, "guardrail", "daily cap, company cooldown or missing materials: stop, or use --force on purpose"),
    (7, "conflict", "duplicate or ambiguous: do not retry blindly, see the detail"),
];
