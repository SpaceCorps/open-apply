use std::path::PathBuf;

use serde_json::json;

use super::Out;
use crate::config::{absolute, home_dir};
use crate::error::{Error, Result};
use crate::readme::SKILL;

/// Writes the Claude Code skill to `<skills dir>/open-apply/SKILL.md`.
/// The skills dir defaults to `~/.claude/skills`.
pub fn install(dir: Option<PathBuf>) -> Result<Out> {
    let root = match dir {
        Some(d) => absolute(&d),
        None => home_dir()
            .ok_or_else(|| Error::validation("cannot find your home directory").hint("pass --dir <skills directory>"))?
            .join(".claude")
            .join("skills"),
    };
    let target_dir = root.join("open-apply");
    let target = target_dir.join("SKILL.md");
    let action = match std::fs::read_to_string(&target) {
        Ok(existing) if existing == SKILL => "unchanged",
        Ok(_) => "updated",
        Err(_) => "created",
    };
    if action != "unchanged" {
        std::fs::create_dir_all(&target_dir)?;
        std::fs::write(&target, SKILL)?;
    }
    Ok(Out::Data(json!({"path": target.to_string_lossy(), "action": action, "version": env!("CARGO_PKG_VERSION")})))
}
