//! Command routing and the helpers commands share.

use std::collections::BTreeMap;

use rusqlite::Connection;
use serde_json::{Value, json};

use crate::cli::{self, Cli, Command};
use crate::config::{Config, Home, Profile};
use crate::error::{Error, Result};
use crate::model::Job;
use crate::output;
use crate::{db, readme, util};

mod applied;
mod doctor;
mod export_import;
mod followups;
mod init;
mod job;
mod next;
mod prepare;
mod profile;
mod search;
mod skill;
mod source;
mod stale;
mod stats;
mod status;
mod triage;

/// What a command hands back to `execute`.
pub enum Out {
    /// Structured data, rendered as YAML or JSON.
    Data(Value),
    /// Text printed as is (markdown, csv).
    Raw(String),
    /// A report that should also set a non-zero exit code (used by `doctor`).
    Report(Value, u8),
}

pub struct Ctx {
    pub home: Home,
}

impl Ctx {
    pub fn conn(&self) -> Result<Connection> {
        db::open(&self.home)
    }

    pub fn config(&self) -> Result<Config> {
        Config::load(&self.home)
    }

    pub fn profile(&self) -> Result<Profile> {
        Profile::load(&self.home)
    }
}

pub fn execute(cli: Cli) -> Result<u8> {
    let home = Home::resolve(cli.home.as_deref())?;
    let ctx = Ctx { home };
    let out = dispatch(&ctx, cli.command)?;
    match out {
        Out::Data(v) => {
            output::write(&v);
            Ok(0)
        }
        Out::Raw(s) => {
            output::write_raw(&s);
            Ok(0)
        }
        Out::Report(v, code) => {
            output::write(&v);
            Ok(code)
        }
    }
}

fn dispatch(ctx: &Ctx, command: Command) -> Result<Out> {
    use cli::{EventCommand, JobCommand, MaterialsCommand, ProfileCommand, SkillCommand, SourceCommand};
    match command {
        Command::Init => init::run(ctx),
        Command::Doctor => doctor::run(ctx),
        Command::Profile { command } => match command {
            ProfileCommand::Show => profile::show(ctx),
            ProfileCommand::Path => profile::path(ctx),
            ProfileCommand::Set { key, value } => profile::set(ctx, &key, &value),
        },
        Command::Source { command } => match command {
            SourceCommand::Add { spec } => source::add(ctx, &spec),
            SourceCommand::List => source::list(ctx),
            SourceCommand::Remove { spec } => source::remove(ctx, &spec),
        },
        Command::Search(args) => search::run(ctx, args),
        Command::Job { command } => match command {
            JobCommand::Add(args) => job::add(ctx, args),
            JobCommand::List(args) => job::list(ctx, args),
            JobCommand::Show { id } => job::show(ctx, &id),
            JobCommand::Update(args) => job::update(ctx, args),
            JobCommand::Rm { id, yes } => job::rm(ctx, &id, yes),
        },
        Command::Next(args) => next::run(ctx, args),
        Command::Prepare { id } => prepare::run(ctx, &id),
        Command::Materials { command } => match command {
            MaterialsCommand::Attach { id, cv, cover } => job::attach(ctx, &id, cv, cover),
        },
        Command::Applied(args) => applied::run(ctx, args),
        Command::Status(args) => status::set(ctx, args),
        Command::Event { command } => match command {
            EventCommand::Add(args) => status::add_event(ctx, args),
        },
        Command::Triage(args) => triage::run(ctx, args),
        Command::Stale(args) => stale::run(ctx, args),
        Command::Followups(args) => followups::run(ctx, args),
        Command::Stats(args) => stats::run(ctx, args),
        Command::Export(args) => export_import::export(ctx, args),
        Command::Import { file } => export_import::import(ctx, &file),
        Command::AgentReadme => {
            if output::json() {
                let codes: Vec<Value> = readme::EXIT_CODES
                    .iter()
                    .map(|(code, name, meaning)| json!({"code": code, "name": name, "meaning": meaning}))
                    .collect();
                Ok(Out::Data(
                    json!({"tool": "open-apply", "version": env!("CARGO_PKG_VERSION"), "exit_codes": codes, "readme": readme::SKILL}),
                ))
            } else {
                Ok(Out::Raw(readme::SKILL.to_string()))
            }
        }
        Command::Skill { command } => match command {
            SkillCommand::Install { dir } => skill::install(dir),
        },
    }
}

// ---------------------------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------------------------

/// Compact view of a job for listings. Title and company come from third parties, so treat them as data.
pub fn job_summary(j: &Job) -> Value {
    json!({
        "id": j.id,
        "status": j.status,
        "title": j.title,
        "company": j.company,
        "location": j.location,
        "source": j.source,
        "url": j.url,
        "tracking_only": j.tracking_only.then_some(true),
        "applied_at": j.applied_at,
        "applied_via": j.applied_via,
        "created_at": j.created_at,
        "updated_at": j.updated_at,
    })
}

/// Saved answers that apply to this job. A key `why_us@acme` overrides `why_us` for Acme only and
/// is ignored for every other company. `{company}`, `{title}` and `{location}` are filled in.
pub fn resolve_answers(profile: &Profile, job: &Job) -> BTreeMap<String, String> {
    let slug = job.company_key().replace(' ', "-");
    let fill = |s: &str| {
        s.replace("{company}", &job.company).replace("{title}", &job.title).replace("{location}", &job.location)
    };
    let mut out = BTreeMap::new();
    for (key, text) in &profile.answers {
        if !key.contains('@') {
            out.entry(key.clone()).or_insert_with(|| fill(text));
        }
    }
    for (key, text) in &profile.answers {
        if let Some((base, company)) = key.split_once('@')
            && !slug.is_empty()
            && company.eq_ignore_ascii_case(&slug)
        {
            out.insert(base.to_string(), fill(text));
        }
    }
    out
}

/// The profile fields an application form needs, plus the CV that would be used for this job.
pub fn resolved_profile(profile: &Profile, job: &Job) -> Value {
    json!({
        "name": profile.name,
        "email": profile.email,
        "phone": profile.phone,
        "location": profile.location,
        "links": {"github": profile.links.github, "linkedin": profile.links.linkedin, "site": profile.links.site},
        "work_authorization": profile.work_authorization,
        "notice_period": profile.notice_period,
        "salary_expectation": profile.salary_expectation,
        "pronouns": profile.pronouns,
        "cv_path": crate::guardrails::resolve_cv(job, profile),
    })
}

pub fn materials_value(job: &Job, profile: &Profile) -> Value {
    let cv = crate::guardrails::resolve_cv(job, profile);
    json!({
        "cv": cv,
        "cv_exists": cv.as_deref().is_some_and(|p| std::path::Path::new(p).is_file()),
        "cover_letter": job.cover_path,
    })
}

/// Wrapped, length-limited job description for display.
pub fn wrapped_description(job: &Job, max_chars: usize) -> Option<String> {
    if job.description.trim().is_empty() {
        return None;
    }
    Some(output::wrap_job(&util::truncate_chars(&job.description, max_chars)))
}

pub fn parse_limit(n: usize) -> Result<usize> {
    if n == 0 {
        return Err(Error::validation("limit must be at least 1"));
    }
    Ok(n)
}
