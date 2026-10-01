//! Guardrails for `applied` and `next`: a cap on applications per rolling 24 hours, a cooldown
//! between roles at the same company, and (optionally) required materials.
//!
//! They exist because low-quality mass-applying hurts the applicant. A value of 0 disables the
//! cap or the cooldown. `--force` overrides all of them and the override is written to the event note.

use rusqlite::Connection;
use serde_json::{Value, json};

use crate::config::{Config, Profile};
use crate::db;
use crate::error::Result;
use crate::model::Job;
use crate::util::{self, DAY};

#[derive(Clone, Debug, PartialEq)]
pub enum Violation {
    DailyCap { applied: u32, cap: u32 },
    Cooldown { job_id: String, title: String, days_apart: f64, cooldown_days: u32 },
    MissingMaterials,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Recent {
    pub job_id: String,
    pub title: String,
    pub days_apart: f64,
}

impl Violation {
    pub fn code(&self) -> &'static str {
        match self {
            Violation::DailyCap { .. } => "daily_cap",
            Violation::Cooldown { .. } => "company_cooldown",
            Violation::MissingMaterials => "missing_materials",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Violation::DailyCap { applied, cap } => {
                format!("daily cap reached: {applied} application(s) in the last 24 hours, cap is {cap}")
            }
            Violation::Cooldown { job_id, title, days_apart, cooldown_days } => format!(
                "company cooldown: applied to '{title}' ({job_id}) at the same company {days_apart:.0} day(s) apart, cooldown is {cooldown_days} day(s)"
            ),
            Violation::MissingMaterials => {
                "require_materials is on and no CV file resolves (attach one with `materials attach` or set profile cv_path)".into()
            }
        }
    }

    pub fn to_value(&self) -> Value {
        json!({"guardrail": self.code(), "message": self.message()})
    }
}

/// Pure decision: given the facts, which guardrails does this application trip?
pub fn evaluate(cfg: &Config, applied_in_window: u32, same_company: &[Recent], has_materials: bool) -> Vec<Violation> {
    let mut out = Vec::new();
    if cfg.daily_application_cap > 0 && applied_in_window >= cfg.daily_application_cap {
        out.push(Violation::DailyCap { applied: applied_in_window, cap: cfg.daily_application_cap });
    }
    if cfg.company_cooldown_days > 0 {
        for r in same_company {
            if r.days_apart < f64::from(cfg.company_cooldown_days) {
                out.push(Violation::Cooldown {
                    job_id: r.job_id.clone(),
                    title: r.title.clone(),
                    days_apart: r.days_apart,
                    cooldown_days: cfg.company_cooldown_days,
                });
            }
        }
    }
    if cfg.require_materials && !has_materials {
        out.push(Violation::MissingMaterials);
    }
    out
}

/// Start of the rolling 24 hour window that ends at `at`.
pub fn window_start(at: &str) -> String {
    util::parse_timestamp(at).map(|t| util::format_rfc3339(t - DAY)).unwrap_or_default()
}

/// CV that would be used for this job: its own attachment, else the profile default.
pub fn resolve_cv(job: &Job, profile: &Profile) -> Option<String> {
    job.cv_path.clone().filter(|p| !p.is_empty()).or_else(|| Some(profile.cv_path.clone()).filter(|p| !p.is_empty()))
}

pub fn has_materials(job: &Job, profile: &Profile) -> bool {
    resolve_cv(job, profile).is_some_and(|p| std::path::Path::new(&p).is_file())
}

/// Other applications to the same company, with the gap in days to `at`.
pub fn recent_same_company(conn: &Connection, job: &Job, at: &str) -> Result<Vec<Recent>> {
    let mut out = Vec::new();
    for (job_id, title, applied_at) in db::company_applications(conn, &job.company_key(), &job.id)? {
        if let Some(d) = util::days_between(&applied_at, at) {
            out.push(Recent { job_id, title, days_apart: d.abs() });
        }
    }
    Ok(out)
}

/// Full check of recording `job` as applied at `at`.
pub fn check(conn: &Connection, cfg: &Config, profile: &Profile, job: &Job, at: &str) -> Result<Vec<Violation>> {
    let applied = db::count_applied_between(conn, &window_start(at), at, Some(&job.id))?;
    let recent = recent_same_company(conn, job, at)?;
    Ok(evaluate(cfg, applied, &recent, has_materials(job, profile)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(cap: u32, cooldown: u32, materials: bool) -> Config {
        Config {
            daily_application_cap: cap,
            company_cooldown_days: cooldown,
            require_materials: materials,
            ..Config::default()
        }
    }

    #[test]
    fn cap_trips_at_the_limit_not_before() {
        assert!(evaluate(&cfg(3, 0, false), 2, &[], true).is_empty());
        let v = evaluate(&cfg(3, 0, false), 3, &[], true);
        assert_eq!(v, vec![Violation::DailyCap { applied: 3, cap: 3 }]);
    }

    #[test]
    fn zero_disables_cap_and_cooldown() {
        let recent = [Recent { job_id: "oa_x".into(), title: "T".into(), days_apart: 1.0 }];
        assert!(evaluate(&cfg(0, 0, false), 1000, &recent, true).is_empty());
    }

    #[test]
    fn cooldown_only_inside_the_window() {
        let recent = [
            Recent { job_id: "oa_a".into(), title: "A".into(), days_apart: 10.0 },
            Recent { job_id: "oa_b".into(), title: "B".into(), days_apart: 120.0 },
        ];
        let v = evaluate(&cfg(25, 90, false), 0, &recent, true);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].code(), "company_cooldown");
        assert!(v[0].message().contains("oa_a"));
    }

    #[test]
    fn materials_requirement() {
        assert!(evaluate(&cfg(25, 90, false), 0, &[], false).is_empty());
        assert_eq!(evaluate(&cfg(25, 90, true), 0, &[], false), vec![Violation::MissingMaterials]);
        assert!(evaluate(&cfg(25, 90, true), 0, &[], true).is_empty());
    }

    #[test]
    fn violations_accumulate() {
        let recent = [Recent { job_id: "oa_a".into(), title: "A".into(), days_apart: 1.0 }];
        let codes: Vec<_> = evaluate(&cfg(1, 90, true), 1, &recent, false).iter().map(Violation::code).collect();
        assert_eq!(codes, ["daily_cap", "company_cooldown", "missing_materials"]);
    }

    #[test]
    fn window_is_24_hours() {
        assert_eq!(window_start("2026-09-02T10:00:00Z"), "2026-09-01T10:00:00Z");
    }
}
