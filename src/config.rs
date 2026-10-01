//! The data home (`~/.open-apply` or `$OPEN_APPLY_HOME`), `config.yaml` (guardrails) and
//! `profile.yaml` (what the agent fills application forms with).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

#[derive(Clone, Debug)]
pub struct Home {
    pub root: PathBuf,
}

impl Home {
    /// Precedence: `--home`, then `$OPEN_APPLY_HOME`, then `~/.open-apply`.
    pub fn resolve(flag: Option<&Path>) -> Result<Home> {
        if let Some(p) = flag {
            return Ok(Home { root: absolute(p) });
        }
        if let Some(v) = std::env::var_os("OPEN_APPLY_HOME").filter(|v| !v.is_empty()) {
            return Ok(Home { root: absolute(Path::new(&v)) });
        }
        let base = home_dir().ok_or_else(|| {
            Error::validation("cannot find your home directory").hint("set OPEN_APPLY_HOME or pass --home PATH")
        })?;
        Ok(Home { root: base.join(".open-apply") })
    }

    pub fn db_path(&self) -> PathBuf {
        self.root.join("open-apply.db")
    }

    pub fn profile_path(&self) -> PathBuf {
        self.root.join("profile.yaml")
    }

    pub fn config_path(&self) -> PathBuf {
        self.root.join("config.yaml")
    }

    pub fn workspaces_dir(&self) -> PathBuf {
        self.root.join("workspaces")
    }

    pub fn workspace(&self, job_id: &str) -> PathBuf {
        self.workspaces_dir().join(job_id)
    }

    pub fn is_initialized(&self) -> bool {
        self.db_path().exists() && self.config_path().exists()
    }

    pub fn require_initialized(&self) -> Result<()> {
        if self.is_initialized() {
            return Ok(());
        }
        Err(Error::not_found(format!("open-apply is not initialized at {}", self.root.display()))
            .hint("run `open-apply init` (or set OPEN_APPLY_HOME / pass --home)"))
    }
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()))
        .map(PathBuf::from)
}

/// Makes a path absolute without touching the filesystem (and without the `\\?\` prefix that
/// `canonicalize` adds on Windows).
pub fn absolute(p: &Path) -> PathBuf {
    if p.is_absolute() {
        return p.to_path_buf();
    }
    match std::env::current_dir() {
        Ok(cwd) => cwd.join(p),
        Err(_) => p.to_path_buf(),
    }
}

// ---------------------------------------------------------------------------------------------
// config.yaml
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    /// Most applications allowed within any rolling 24 hours.
    pub daily_application_cap: u32,
    /// Days to wait before applying to another role at a company already applied to.
    pub company_cooldown_days: u32,
    /// When true, `applied` and `next` require a resolvable CV file.
    pub require_materials: bool,
    /// Follow-ups suggested per application before `followups` stops listing it.
    pub max_followups: u32,
    /// Timeout for each HTTP request, in seconds.
    pub http_timeout_secs: u64,
    /// Minimum pause between two requests to the same host, in milliseconds.
    pub request_delay_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            daily_application_cap: 25,
            company_cooldown_days: 90,
            require_materials: false,
            max_followups: 2,
            http_timeout_secs: 20,
            request_delay_ms: 500,
        }
    }
}

pub const CONFIG_HEADER: &str = "# open-apply guardrails. Mass-applying with low-quality materials hurts the applicant,\n# so `applied` and `next` stop at these limits (exit code 6). `--force` overrides them.\n";

impl Config {
    pub fn load(home: &Home) -> Result<Config> {
        let path = home.config_path();
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
            Err(e) => return Err(e.into()),
        };
        if text.trim().is_empty() {
            return Ok(Config::default());
        }
        serde_norway::from_str(&text).map_err(|e| {
            Error::validation(format!("{} is not valid: {e}", path.display()))
                .hint("fix the YAML, or delete the file and run `open-apply init` to recreate the defaults")
        })
    }

    pub fn to_yaml(&self) -> Result<String> {
        Ok(format!("{CONFIG_HEADER}{}", serde_norway::to_string(self)?))
    }

    /// Delay between requests to one host; `OPEN_APPLY_REQUEST_DELAY_MS` overrides (tests set 0).
    pub fn request_delay_ms(&self) -> u64 {
        std::env::var("OPEN_APPLY_REQUEST_DELAY_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(self.request_delay_ms)
    }
}

// ---------------------------------------------------------------------------------------------
// profile.yaml
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Links {
    pub github: String,
    pub linkedin: String,
    pub site: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Profile {
    pub name: String,
    pub email: String,
    pub phone: String,
    pub location: String,
    pub links: Links,
    pub cv_path: String,
    pub work_authorization: String,
    pub notice_period: String,
    pub salary_expectation: String,
    pub pronouns: String,
    /// Reusable answers to screening questions, keyed by a short question key.
    pub answers: BTreeMap<String, String>,
}

pub const PROFILE_HEADER: &str = "# open-apply profile. The agent reads this to fill application forms.\n# Edit by hand or with `open-apply profile set <key> <value>`.\n# Keys: name, email, phone, location, links.github, links.linkedin, links.site, cv_path,\n#       work_authorization, notice_period, salary_expectation, pronouns, answers.<question-key>\n# Answers may use {company}, {title} and {location}; `open-apply next` fills them in per job.\n";

pub const PROFILE_KEYS: [&str; 12] = [
    "name",
    "email",
    "phone",
    "location",
    "links.github",
    "links.linkedin",
    "links.site",
    "cv_path",
    "work_authorization",
    "notice_period",
    "salary_expectation",
    "pronouns",
];

pub fn valid_keys_hint() -> String {
    format!("valid keys: {}, answers.<question-key>", PROFILE_KEYS.join(", "))
}

impl Profile {
    pub fn load(home: &Home) -> Result<Profile> {
        let path = home.profile_path();
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Profile::default()),
            Err(e) => return Err(e.into()),
        };
        if text.trim().is_empty() {
            return Ok(Profile::default());
        }
        serde_norway::from_str(&text).map_err(|e| {
            Error::validation(format!("{} is not valid: {e}", path.display())).hint("fix the YAML by hand")
        })
    }

    pub fn save(&self, home: &Home) -> Result<()> {
        let text = format!("{PROFILE_HEADER}{}", serde_norway::to_string(self)?);
        write_atomic(&home.profile_path(), &text)
    }

    /// Sets a dotted key. An empty value clears it (and removes an answer).
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        let value = value.trim();
        if let Some(q) = key.strip_prefix("answers.") {
            let q = q.trim();
            if q.is_empty() || !q.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '@' | '.')) {
                return Err(Error::validation(format!("bad answer key '{key}'"))
                    .hint("use letters, digits, '_' or '-', for example answers.why_this_company"));
            }
            if value.is_empty() {
                self.answers.remove(q);
            } else {
                self.answers.insert(q.to_string(), value.to_string());
            }
            return Ok(());
        }
        if key.starts_with("links.")
            && !value.is_empty()
            && !(value.starts_with("http://") || value.starts_with("https://"))
        {
            return Err(Error::validation(format!("{key} must be a full http(s) URL")));
        }
        if key == "email" && !value.is_empty() && (!value.contains('@') || value.contains(char::is_whitespace)) {
            return Err(Error::validation("email must look like name@example.com"));
        }
        let slot = match key {
            "name" => &mut self.name,
            "email" => &mut self.email,
            "phone" => &mut self.phone,
            "location" => &mut self.location,
            "links.github" => &mut self.links.github,
            "links.linkedin" => &mut self.links.linkedin,
            "links.site" => &mut self.links.site,
            "cv_path" => &mut self.cv_path,
            "work_authorization" => &mut self.work_authorization,
            "notice_period" => &mut self.notice_period,
            "salary_expectation" => &mut self.salary_expectation,
            "pronouns" => &mut self.pronouns,
            _ => return Err(Error::validation(format!("unknown profile key '{key}'")).hint(valid_keys_hint())),
        };
        *slot = if key == "cv_path" && !value.is_empty() {
            absolute(Path::new(value)).to_string_lossy().into_owned()
        } else {
            value.to_string()
        };
        Ok(())
    }

    /// Missing fields an agent needs before it can fill a typical application form.
    pub fn missing_required(&self) -> Vec<&'static str> {
        let mut m = Vec::new();
        if self.name.trim().is_empty() {
            m.push("name");
        }
        if self.email.trim().is_empty() {
            m.push("email");
        }
        m
    }

    pub fn missing_recommended(&self) -> Vec<&'static str> {
        let mut m = Vec::new();
        if self.location.trim().is_empty() {
            m.push("location");
        }
        if self.cv_path.trim().is_empty() {
            m.push("cv_path");
        }
        if self.work_authorization.trim().is_empty() {
            m.push("work_authorization");
        }
        m
    }
}

/// Writes a file via a temporary sibling so a crash never leaves a half-written YAML behind.
pub fn write_atomic(path: &Path, text: &str) -> Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_spec() {
        let c = Config::default();
        assert_eq!((c.daily_application_cap, c.company_cooldown_days, c.require_materials), (25, 90, false));
    }

    #[test]
    fn config_partial_yaml_uses_defaults() {
        let c: Config = serde_norway::from_str("daily_application_cap: 3\n").unwrap();
        assert_eq!(c.daily_application_cap, 3);
        assert_eq!(c.company_cooldown_days, 90);
    }

    #[test]
    fn config_rejects_unknown_types() {
        assert!(serde_norway::from_str::<Config>("daily_application_cap: lots\n").is_err());
    }

    #[test]
    fn profile_set_scalars_links_answers() {
        let mut p = Profile::default();
        p.set("name", "Ada Lovelace").unwrap();
        p.set("links.github", "https://github.com/ada").unwrap();
        p.set("answers.why_us", "I like {company}.").unwrap();
        assert_eq!(p.name, "Ada Lovelace");
        assert_eq!(p.links.github, "https://github.com/ada");
        assert_eq!(p.answers["why_us"], "I like {company}.");
        p.set("answers.why_us", "").unwrap();
        assert!(p.answers.is_empty());
    }

    #[test]
    fn profile_set_validates() {
        let mut p = Profile::default();
        assert!(p.set("email", "nope").is_err());
        assert!(p.set("links.github", "github.com/ada").is_err());
        assert!(p.set("shoe_size", "42").is_err());
        assert!(p.set("answers.bad key", "x").is_err());
    }

    #[test]
    fn missing_fields() {
        let mut p = Profile::default();
        assert_eq!(p.missing_required(), vec!["name", "email"]);
        p.name = "A".into();
        p.email = "a@b.c".into();
        assert!(p.missing_required().is_empty());
        assert_eq!(p.missing_recommended(), vec!["location", "cv_path", "work_authorization"]);
    }
}
