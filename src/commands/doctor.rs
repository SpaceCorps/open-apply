use rusqlite::Connection;
use serde_json::{Value, json};

use super::{Ctx, Out};
use crate::config::{Config, Profile};
use crate::db;
use crate::error::{ErrorCode, Result};

struct Checks {
    items: Vec<Value>,
}

impl Checks {
    fn push(&mut self, name: &str, status: &str, detail: impl Into<String>, hint: Option<&str>) {
        self.items.push(json!({"check": name, "status": status, "detail": detail.into(), "hint": hint}));
    }

    fn ok(&mut self, name: &str, detail: impl Into<String>) {
        self.push(name, "ok", detail, None);
    }

    fn warn(&mut self, name: &str, detail: impl Into<String>, hint: &str) {
        self.push(name, "warn", detail, Some(hint));
    }

    fn fail(&mut self, name: &str, detail: impl Into<String>, hint: &str) {
        self.push(name, "fail", detail, Some(hint));
    }

    fn count(&self, status: &str) -> usize {
        self.items.iter().filter(|i| i["status"] == status).count()
    }
}

pub fn run(ctx: &Ctx) -> Result<Out> {
    let home = &ctx.home;
    let mut c = Checks { items: Vec::new() };

    if home.root.is_dir() {
        c.ok("home", home.root.to_string_lossy());
    } else {
        c.fail("home", format!("{} does not exist", home.root.display()), "run `open-apply init`");
    }

    match Config::load(home) {
        Ok(cfg) => {
            if home.config_path().exists() {
                c.ok(
                    "config",
                    format!(
                        "daily_application_cap={} company_cooldown_days={} require_materials={}",
                        cfg.daily_application_cap, cfg.company_cooldown_days, cfg.require_materials
                    ),
                );
            } else {
                c.fail("config", "config.yaml is missing", "run `open-apply init`");
            }
        }
        Err(e) => c.fail("config", e.message, "fix config.yaml or delete it and run `open-apply init`"),
    }

    let mut conn: Option<Connection> = None;
    if !home.db_path().exists() {
        c.fail("database", "open-apply.db is missing", "run `open-apply init`");
    } else {
        match db::open(home) {
            Ok(co) => {
                let version = db::schema_version(&co)?;
                if version == db::latest_schema_version() {
                    c.ok("schema", format!("user_version {version} (latest)"));
                } else {
                    c.fail(
                        "schema",
                        format!("user_version {version}, expected {}", db::latest_schema_version()),
                        "run any command to migrate",
                    );
                }
                let mode: String = co.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap_or_default();
                if mode.eq_ignore_ascii_case("wal") {
                    c.ok("journal_mode", "wal");
                } else {
                    c.warn("journal_mode", format!("{mode}, expected wal"), "the filesystem may not support WAL");
                }
                let integrity: String = co.query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap_or_default();
                if integrity == "ok" {
                    c.ok("integrity", "sqlite integrity_check ok");
                } else {
                    c.fail("integrity", integrity, "restore from an export: `open-apply import`");
                }
                conn = Some(co);
            }
            Err(e) => c.fail("database", e.message, "check the file is not locked or corrupt"),
        }
    }

    if let Some(co) = &conn {
        let jobs: i64 = co.query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get(0)).unwrap_or(0);
        let events: i64 = co.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0)).unwrap_or(0);
        let sources: i64 = co.query_row("SELECT COUNT(*) FROM sources", [], |r| r.get(0)).unwrap_or(0);
        c.ok("data", format!("{jobs} job(s), {events} event(s), {sources} watched source(s)"));
        if sources == 0 {
            c.warn(
                "sources",
                "no watched feeds",
                "run `open-apply source add greenhouse:<board>` or pass --source to `search`",
            );
        }
    }

    match Profile::load(home) {
        Ok(p) => {
            let missing = p.missing_required();
            if !home.profile_path().exists() {
                c.fail("profile", "profile.yaml is missing", "run `open-apply init`");
            } else if missing.is_empty() {
                c.ok("profile", "name and email are set");
            } else {
                c.fail(
                    "profile",
                    format!("missing: {}", missing.join(", ")),
                    "run `open-apply profile set <key> <value>`",
                );
            }
            let rec = p.missing_recommended();
            if !rec.is_empty() && home.profile_path().exists() {
                c.warn(
                    "profile_recommended",
                    format!("not set: {}", rec.join(", ")),
                    "agents fill forms better with these",
                );
            }
            if !p.cv_path.is_empty() {
                if std::path::Path::new(&p.cv_path).is_file() {
                    c.ok("cv", p.cv_path.clone());
                } else {
                    c.warn(
                        "cv",
                        format!("{} does not exist", p.cv_path),
                        "fix with `open-apply profile set cv_path <path>`",
                    );
                }
            }
        }
        Err(e) => c.fail("profile", e.message, "fix profile.yaml by hand"),
    }

    let fails = c.count("fail");
    let report = json!({
        "ok": fails == 0,
        "home": home.root.to_string_lossy(),
        "summary": {"ok": c.count("ok"), "warn": c.count("warn"), "fail": fails},
        "checks": c.items,
    });
    if fails == 0 { Ok(Out::Data(report)) } else { Ok(Out::Report(report, ErrorCode::Validation as u8)) }
}
