use serde_json::json;

use super::{Ctx, Out};
use crate::config::{Config, Profile, write_atomic};
use crate::db;
use crate::error::Result;

pub fn run(ctx: &Ctx) -> Result<Out> {
    let home = &ctx.home;
    let mut created = Vec::new();
    let mut existing = Vec::new();

    std::fs::create_dir_all(home.workspaces_dir())?;

    let had_db = home.db_path().exists();
    let conn = db::init(home)?;
    let version = db::schema_version(&conn)?;
    if had_db {
        existing.push("open-apply.db")
    } else {
        created.push("open-apply.db")
    }

    if home.config_path().exists() {
        existing.push("config.yaml");
    } else {
        write_atomic(&home.config_path(), &Config::default().to_yaml()?)?;
        created.push("config.yaml");
    }
    if home.profile_path().exists() {
        existing.push("profile.yaml");
    } else {
        Profile::default().save(home)?;
        created.push("profile.yaml");
    }

    let profile = Profile::load(home)?;
    Ok(Out::Data(json!({
        "home": home.root.to_string_lossy(),
        "created": created,
        "already_present": existing,
        "schema_version": version,
        "profile_missing": profile.missing_required(),
        "next_steps": [
            "open-apply profile set name \"Your Name\"",
            "open-apply profile set email you@example.com",
            "open-apply doctor",
        ],
    })))
}
