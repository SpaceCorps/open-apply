use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "open-apply",
    author = "SpaceCorps",
    version,
    about = "Job search CLI built for LLM agents - discover postings, prepare materials, record every application, track responses",
    subcommand_required = true,
    arg_required_else_help = true
)]
pub struct Cli {
    #[arg(long, global = true, help = "Emit JSON instead of YAML")]
    pub json: bool,

    #[arg(long, global = true, help = "Suppress progress and warning messages on stderr")]
    pub quiet: bool,

    #[arg(long, global = true, value_name = "PATH", help = "Data home (default: $OPEN_APPLY_HOME or ~/.open-apply)")]
    pub home: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    #[command(about = "Create the data home with a profile template and default config")]
    Init,

    #[command(about = "Check the home, database, schema and profile completeness")]
    Doctor,

    #[command(about = "Show or change your profile (what the agent fills forms with)")]
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },

    #[command(about = "Manage watched public job feeds")]
    Source {
        #[command(subcommand)]
        command: SourceCommand,
    },

    #[command(about = "Search public job feeds (no login, no scraping)")]
    Search(SearchArgs),

    #[command(about = "Add, list, show, update and remove jobs")]
    Job {
        #[command(subcommand)]
        command: JobCommand,
    },

    #[command(about = "The agent work queue: next jobs to act on, with profile fields and saved answers")]
    Next(NextArgs),

    #[command(about = "Create the workspace for a job: job.md, profile.json, answers.json, cover-letter.md")]
    Prepare {
        #[arg(value_name = "ID")]
        id: String,
    },

    #[command(about = "Attach a CV and cover letter to a job")]
    Materials {
        #[command(subcommand)]
        command: MaterialsCommand,
    },

    #[command(about = "Record that you applied (enforces the daily cap and company cooldown)")]
    Applied(AppliedArgs),

    #[command(about = "Set a job's status by hand")]
    Status(StatusArgs),

    #[command(about = "Append an event to a job's history")]
    Event {
        #[command(subcommand)]
        command: EventCommand,
    },

    #[command(about = "Classify an inbound employer email and match it to an application")]
    Triage(TriageArgs),

    #[command(about = "List or mark applications that never got a response")]
    Stale(StaleArgs),

    #[command(about = "Applications due a polite follow-up")]
    Followups(FollowupsArgs),

    #[command(about = "Funnel counts, response rate and response time")]
    Stats(StatsArgs),

    #[command(about = "Export jobs and history as json, csv or md")]
    Export(ExportArgs),

    #[command(about = "Import a JSON export or a CSV of jobs")]
    Import {
        #[arg(value_name = "FILE")]
        file: PathBuf,
    },

    #[command(name = "agent-readme", about = "Print the operating manual for an LLM agent")]
    AgentReadme,

    #[command(about = "Install the Claude Code skill")]
    Skill {
        #[command(subcommand)]
        command: SkillCommand,
    },
}

#[derive(Subcommand, Debug)]
pub enum ProfileCommand {
    #[command(about = "Print the profile")]
    Show,
    #[command(about = "Print the path of profile.yaml")]
    Path,
    #[command(
        about = "Set a field, e.g. `profile set links.github https://github.com/you` or `profile set answers.why_us \"...\"`"
    )]
    Set {
        #[arg(value_name = "KEY")]
        key: String,
        #[arg(value_name = "VALUE")]
        value: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum SourceCommand {
    #[command(
        about = "Watch a feed: greenhouse:<board>, lever:<company>, ashby:<org>, remoteok, weworkremotely, arbeitnow"
    )]
    Add {
        #[arg(value_name = "KIND:IDENT")]
        spec: String,
    },
    #[command(about = "List watched feeds")]
    List,
    #[command(about = "Stop watching a feed")]
    Remove {
        #[arg(value_name = "KIND:IDENT")]
        spec: String,
    },
}

#[derive(Args, Debug)]
pub struct SearchArgs {
    #[arg(
        long = "source",
        value_name = "KIND:IDENT",
        help = "Feed to query (repeatable); default: your watched feeds"
    )]
    pub sources: Vec<String>,

    #[arg(long, value_name = "TEXT", help = "Every word must appear in title, company, tags or description")]
    pub query: Option<String>,

    #[arg(long, value_name = "TEXT", help = "Location must contain this text")]
    pub location: Option<String>,

    #[arg(long, help = "Only remote postings")]
    pub remote: bool,

    #[arg(long, value_name = "N", default_value_t = 25, help = "Maximum results (sampled across feeds)")]
    pub limit: usize,

    #[arg(long, help = "Store results as `lead` jobs, skipping duplicates")]
    pub save: bool,
}

#[derive(Subcommand, Debug)]
pub enum JobCommand {
    #[command(about = "Add a job by URL (ATS pages and JobPosting JSON-LD are read automatically)")]
    Add(JobAddArgs),
    #[command(about = "List jobs")]
    List(JobListArgs),
    #[command(about = "Show one job with its full history")]
    Show {
        #[arg(value_name = "ID")]
        id: String,
    },
    #[command(about = "Edit a job's fields")]
    Update(JobUpdateArgs),
    #[command(about = "Delete a job and its history")]
    Rm {
        #[arg(value_name = "ID")]
        id: String,
        #[arg(long, help = "Confirm the deletion")]
        yes: bool,
    },
}

#[derive(Args, Debug)]
pub struct JobAddArgs {
    #[arg(value_name = "URL")]
    pub url: String,
    #[arg(long)]
    pub title: Option<String>,
    #[arg(long)]
    pub company: Option<String>,
    #[arg(long)]
    pub location: Option<String>,
    #[arg(long, help = "Where you found it (default: manual)")]
    pub source: Option<String>,
    #[arg(long)]
    pub notes: Option<String>,
    #[arg(long, help = "Do not fetch the page or the ATS endpoint; store what was given")]
    pub no_fetch: bool,
    #[arg(long, help = "Add even if the same company, title and location is already stored")]
    pub allow_duplicate: bool,
}

#[derive(Args, Debug)]
pub struct JobListArgs {
    #[arg(long, value_name = "STATUS", help = "Only this status (comma-separated list allowed)")]
    pub status: Option<String>,
    #[arg(long, value_name = "TEXT", help = "Company name contains this text")]
    pub company: Option<String>,
    #[arg(long, value_name = "DATE", help = "Added on or after this date")]
    pub since: Option<String>,
    #[arg(long, value_name = "N", default_value_t = 50)]
    pub limit: usize,
}

#[derive(Args, Debug)]
pub struct JobUpdateArgs {
    #[arg(value_name = "ID")]
    pub id: String,
    #[arg(long)]
    pub title: Option<String>,
    #[arg(long)]
    pub company: Option<String>,
    #[arg(long)]
    pub location: Option<String>,
    #[arg(long)]
    pub notes: Option<String>,
    #[arg(long)]
    pub source: Option<String>,
    #[arg(long, value_name = "DATE", help = "Correct the application date (job must already be applied)")]
    pub applied_at: Option<String>,
    #[arg(long, value_name = "CHANNEL", help = "Correct the channel: linkedin, ats, email, referral, other")]
    pub via: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum MaterialsCommand {
    #[command(about = "Record the CV (and optionally cover letter) to use for a job")]
    Attach {
        #[arg(value_name = "ID")]
        id: String,
        #[arg(long, value_name = "PATH")]
        cv: Option<PathBuf>,
        #[arg(long, value_name = "PATH")]
        cover: Option<PathBuf>,
    },
}

#[derive(Args, Debug)]
pub struct NextArgs {
    #[arg(long, value_name = "N", default_value_t = 3)]
    pub count: usize,
    #[arg(long, help = "Ignore the daily cap, company cooldown and materials requirement")]
    pub force: bool,
}

#[derive(Args, Debug)]
pub struct AppliedArgs {
    #[arg(value_name = "ID")]
    pub id: String,
    #[arg(
        long,
        value_name = "CHANNEL",
        help = "linkedin, ats, email, referral or other (default: inferred from the URL)"
    )]
    pub via: Option<String>,
    #[arg(long, value_name = "DATE", help = "When you applied (default: now)")]
    pub at: Option<String>,
    #[arg(long)]
    pub note: Option<String>,
    #[arg(long, help = "Override the guardrails (recorded in the event note)")]
    pub force: bool,
}

#[derive(Args, Debug)]
pub struct StatusArgs {
    #[arg(value_name = "ID")]
    pub id: String,
    #[arg(value_name = "STATUS")]
    pub status: String,
    #[arg(long)]
    pub note: Option<String>,
    #[arg(long, value_name = "DATE")]
    pub at: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum EventCommand {
    #[command(
        about = "Record something that happened: ack, screen, interview, assessment, offer, rejection, follow_up, note"
    )]
    Add(EventAddArgs),
}

#[derive(Args, Debug)]
pub struct EventAddArgs {
    #[arg(value_name = "ID")]
    pub id: String,
    #[arg(long = "type", value_name = "TYPE")]
    pub kind: String,
    #[arg(long)]
    pub note: Option<String>,
    #[arg(long, value_name = "DATE")]
    pub at: Option<String>,
}

#[derive(Args, Debug)]
pub struct TriageArgs {
    #[arg(long, value_name = "ADDR", help = "Sender, e.g. \"Jane <jane@acme.com>\"")]
    pub from: Option<String>,
    #[arg(long, value_name = "TEXT")]
    pub subject: Option<String>,
    #[arg(long, help = "Read the email body from stdin")]
    pub stdin: bool,
    #[arg(long, help = "Record the event and advance the status (otherwise a dry run)")]
    pub apply: bool,
    #[arg(long, value_name = "ID", help = "Use this application instead of matching")]
    pub job: Option<String>,
    #[arg(long, value_name = "DATE", help = "When the email arrived (default: now)")]
    pub at: Option<String>,
}

#[derive(Args, Debug)]
pub struct StaleArgs {
    #[arg(long, value_name = "N", default_value_t = 21)]
    pub days: u32,
    #[arg(long, help = "Mark them ghosted")]
    pub mark: bool,
}

#[derive(Args, Debug)]
pub struct FollowupsArgs {
    #[arg(long, value_name = "N", default_value_t = 7)]
    pub days: u32,
}

#[derive(Args, Debug)]
pub struct StatsArgs {
    #[arg(long, value_name = "DATE")]
    pub since: Option<String>,
}

#[derive(Args, Debug)]
pub struct ExportArgs {
    #[arg(long, value_name = "FORMAT", default_value = "json", help = "json, csv or md")]
    pub format: String,
    #[arg(long, value_name = "PATH", help = "Write to a file instead of stdout")]
    pub out: Option<PathBuf>,
}

#[derive(Subcommand, Debug)]
pub enum SkillCommand {
    #[command(about = "Write the skill to ~/.claude/skills/open-apply/SKILL.md")]
    Install {
        #[arg(
            long,
            value_name = "PATH",
            help = "Skills directory to install into (the skill goes in PATH/open-apply/)"
        )]
        dir: Option<PathBuf>,
    },
}
