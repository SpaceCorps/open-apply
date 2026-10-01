//! `open-apply` - a job search CLI built to be driven by an LLM agent.
//!
//! Discovers postings from public feeds, prepares per-job materials, records every application
//! and tracks whether employers respond. It never submits an application itself.

mod cli;
mod commands;
mod config;
mod db;
mod error;
mod export;
mod guardrails;
mod model;
mod output;
mod readme;
mod sources;
mod stats;
mod triage;
mod url;
mod util;

use std::process::ExitCode;

use clap::Parser;
use clap::error::ErrorKind;

use crate::error::{Error, ErrorCode};

fn main() -> ExitCode {
    // The error envelope can be rendered before parsing succeeds, so the format has to be known
    // from the raw args first.
    let json_prescan = std::env::args_os().skip(1).take_while(|a| a != "--").any(|a| a == "--json");
    output::set_json(json_prescan);

    let cli = match cli::Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => return parse_error(e),
    };

    output::set_json(cli.json);
    output::set_quiet(cli.quiet);

    match commands::execute(cli) {
        Ok(code) => ExitCode::from(code),
        Err(e) => ExitCode::from(output::write_error(&e) as u8),
    }
}

/// Help and version are successes; everything else clap refuses is a `usage` envelope,
/// with clap's own explanation as the message and its usage line as the hint.
fn parse_error(e: clap::Error) -> ExitCode {
    match e.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
            let _ = e.print();
            ExitCode::SUCCESS
        }
        ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
            let _ = e.print();
            ExitCode::from(ErrorCode::Usage as u8)
        }
        _ => {
            let rendered = e.render().to_string();
            let (what, usage) = rendered.split_once("\n\n").unwrap_or((&rendered, ""));
            let message = what
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
                .trim_start_matches("error: ")
                .to_string();
            let mut err = Error::usage(if message.is_empty() { "Invalid arguments.".into() } else { message });
            let usage: Vec<&str> = usage.lines().map(str::trim).filter(|l| l.starts_with("Usage:")).collect();
            err = err.hint(if usage.is_empty() {
                "run with --help".to_string()
            } else {
                format!("{} (run with --help for details)", usage[0])
            });
            ExitCode::from(output::write_error(&err) as u8)
        }
    }
}
