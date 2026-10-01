# open-apply

[![Release](https://img.shields.io/github/v/release/SpaceCorps/open-apply?color=blue&label=version)](https://github.com/SpaceCorps/open-apply/releases/latest)
[![CI](https://github.com/SpaceCorps/open-apply/actions/workflows/ci.yml/badge.svg)](https://github.com/SpaceCorps/open-apply/actions/workflows/ci.yml)
[![Docs](https://img.shields.io/badge/docs-online-success)](https://spacecorps.github.io/open-apply/)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A command-line tool and agent interface for running a job search. It finds postings in legitimate public feeds, prepares materials per job, records every application, and tracks whether and when employers answer. Built in Rust 2024 so an AI agent such as Claude Code can drive it end to end, and so a person can read what it did.

`open-apply` is the system of record. It does not apply for you: you (or your agent) apply in a browser, on LinkedIn, a company ATS or by email, and `open-apply` supplies the data for the form and records the result.

---

## Highlights

- **System of record for the whole search**: jobs, materials, applications and employer replies live in one local SQLite database, with an append-only history of every status change.
- **Public sources only**: Greenhouse, Lever and Ashby boards, RemoteOK, We Work Remotely and Arbeitnow, through their documented unauthenticated endpoints, with an honest `User-Agent`, time-outs and a per-host politeness delay.
- **Add any posting by URL**: Greenhouse, Lever and Ashby links are read through their public JSON; other pages through schema.org `JobPosting` data when the page has it. LinkedIn, Indeed and similar links are tracked, never fetched.
- **Agent work queue**: `next` returns the jobs to act on with the profile fields, saved screening answers (filled in per company) and materials paths an application form needs.
- **Guardrails against low-quality mass-applying**: a daily cap, a per-company cooldown and an optional materials requirement, enforced where the application is recorded. `--force` overrides and leaves a mark in the history.
- **Reply triage**: a rule-based classifier reads an inbound employer email (ack, screen, interview, assessment, offer, rejection, noise), matches it to an application and, with `--apply`, records the event and advances the status.
- **Follow-ups and stats**: who is due a nudge, who never answered, response rate, median days to first response, and breakdowns by source and by channel.
- **Prompt-injection aware**: job descriptions and emails are always wrapped in explicit delimiters when echoed, and nothing in them is ever executed.
- **Machine-readable output**: YAML on stdout by default, JSON with `--json`, structured error envelopes and stable exit codes.
- **One binary**: SQLite is bundled and TLS is rustls, so there is nothing else to install.

---

## Installation

### Using Cargo

```bash
cargo install --git https://github.com/SpaceCorps/open-apply --locked
```

### Pre-built binaries

Tagged releases attach `open-apply-<tag>-<target>` archives built by [`release.yml`](.github/workflows/release.yml) for:

| Platform | Architecture |
|:---|:---|
| macOS | Apple Silicon (`aarch64-apple-darwin`) |
| macOS | Intel (`x86_64-apple-darwin`) |
| Linux | x86_64 musl (`x86_64-unknown-linux-musl`) |
| Linux | aarch64 musl (`aarch64-unknown-linux-musl`) |
| Windows | x64 MSVC (`x86_64-pc-windows-msvc`) |

No release has been cut yet, so build from source for now.

### Use it from Claude Code

```bash
open-apply skill install     # writes ~/.claude/skills/open-apply/SKILL.md
open-apply agent-readme      # prints the same manual
```

---

## Quickstart

The transcript below is real output from the release binary, with the data home abbreviated to `~/.open-apply` (captured on Windows 11, so paths inside the home use backslashes there). Example Co, Initech and Hooli are made up; the search in step 2 hit the live Arbeitnow API.

### 1. Set up

```console
$ open-apply init
home: ~/.open-apply
created:
- open-apply.db
- config.yaml
- profile.yaml
already_present: []
schema_version: 1
profile_missing:
- name
- email
next_steps:
- open-apply profile set name "Your Name"
- open-apply profile set email you@example.com
- open-apply doctor

$ open-apply profile set name "Ada Lovelace"
$ open-apply profile set email ada@example.com
set: email
cleared: false
missing_required: []
missing_recommended:
- location
- cv_path
- work_authorization

$ open-apply profile set answers.why_us "I want to work on {title} at {company}; I have shipped a similar system before."
$ open-apply doctor
ok: true
home: ~/.open-apply
summary:
  ok: 7
  warn: 2
  fail: 0
checks:
- check: home
  status: ok
  detail: ~/.open-apply
- check: config
  status: ok
  detail: daily_application_cap=25 company_cooldown_days=90 require_materials=false
- check: schema
  status: ok
  detail: user_version 1 (latest)
- check: journal_mode
  status: ok
  detail: wal
- check: integrity
  status: ok
  detail: sqlite integrity_check ok
- check: data
  status: ok
  detail: 0 job(s), 0 event(s), 0 watched source(s)
- check: sources
  status: warn
  detail: no watched feeds
  hint: run `open-apply source add greenhouse:<board>` or pass --source to `search`
- check: profile
  status: ok
  detail: name and email are set
- check: profile_recommended
  status: warn
  detail: 'not set: location, cv_path, work_authorization'
  hint: agents fill forms better with these
```

(The output of the other `profile set` calls is left out; it has the same shape as the one shown.)

Data lives in `$OPEN_APPLY_HOME` or `~/.open-apply`: the SQLite database (`open-apply.db`, WAL mode), `profile.yaml`, `config.yaml` and one workspace folder per job.

### 2. Find jobs

```console
$ open-apply source add arbeitnow
source: arbeitnow
added: true
already_watched: false

$ open-apply search --query python --limit 1 --save
query: python
sources:
- source: arbeitnow
  fetched: 326
  matched: 62
matched: 62
count: 1
results:
- title: DevOps Enginer (m/w/d)- Kubernetes & AI Infrastructure
  company: ADITO Software GmbH
  location: Landshut
  remote: false
  url: https://www.arbeitnow.com/jobs/companies/adito-software-gmbh/devops-enginer-kubernetes-ai-infrastructure-landshut-328134
  source: arbeitnow
  posted_at: 2026-10-01T10:30:28Z
  id: oa_31f7f232
  saved: true
saved: 1
duplicates: 0
hint: saved leads are queued for `open-apply next`
```

Results are stored as `lead` jobs. Running the same search again reports duplicates instead of adding them. Watch a company with `open-apply source add greenhouse:<board>` (or `lever:<company>`, `ashby:<org>`), or add one posting by URL with `open-apply job add <url>`.

### 3. Prepare and apply

```console
$ open-apply job add https://example.com/jobs/senior-rust-engineer --no-fetch --title "Senior Rust Engineer" --company "Example Co" --location "Remote (EU)"
job:
  id: oa_ac9b7206
  status: saved
  title: Senior Rust Engineer
  company: Example Co
  location: Remote (EU)
  source: manual
  url: https://example.com/jobs/senior-rust-engineer
  created_at: 2026-10-01T11:41:08Z
  updated_at: 2026-10-01T11:41:08Z
fetched: false
description_chars: 0
next: open-apply prepare oa_ac9b7206

$ open-apply prepare oa_ac9b7206
id: oa_ac9b7206
workspace: ~/.open-apply\workspaces\oa_ac9b7206
files:
- job.md
- profile.json
- answers.json
- cover-letter.md
cover_letter_created: true
status: ready
status_changed: true
materials:
  cv_exists: false
missing_profile: []
next: write ~/.open-apply\workspaces\oa_ac9b7206\cover-letter.md then apply in a browser, then run `open-apply applied oa_ac9b7206`
```

`prepare` creates `workspaces/oa_ac9b7206/` with `job.md` (the posting, inside untrusted-content delimiters), `profile.json`, `answers.json` and an empty `cover-letter.md`. The agent writes the letter, applies in a browser, then:

```console
$ open-apply next --count 1
guardrails:
  daily_application_cap: 25
  applied_last_24h: 0
  remaining_today: 25
  company_cooldown_days: 90
  require_materials: false
  max_followups: 2
count: 1
jobs:
- id: oa_ac9b7206
  status: ready
  title: Senior Rust Engineer
  company: Example Co
  location: Remote (EU)
  url: https://example.com/jobs/senior-rust-engineer
  source: manual
  apply_via: other
  workspace: ~/.open-apply\workspaces\oa_ac9b7206
  materials:
    cv_exists: false
  profile:
    name: Ada Lovelace
    email: ada@example.com
    phone: ''
    location: ''
    links:
      github: ''
      linkedin: ''
      site: ''
    work_authorization: ''
    notice_period: ''
    salary_expectation: ''
    pronouns: ''
  answers:
    why_us: I want to work on Senior Rust Engineer at Example Co; I have shipped a similar system before.

$ open-apply applied oa_ac9b7206 --via ats --note "cover letter v1"
id: oa_ac9b7206
status: applied
company: Example Co
title: Senior Rust Engineer
applied_at: 2026-10-01T11:41:08Z
via: ats
applied_last_24h: 1
remaining_today: 24
hint: if nothing comes back, `open-apply followups` and `open-apply stale` will surface oa_ac9b7206
```

### 4. Record replies

Pipe an employer email into `triage`. Without `--apply` it is a dry run that prints the same analysis with `applied: false`; with `--apply` it appends the event and advances the status. Here `email.txt` holds the body of the recruiter's message.

```console
$ open-apply triage --from "Example Co Recruiting <jobs@example.com>" --subject "Thank you for applying to Example Co" --stdin --apply < email.txt
email: |-
  --- untrusted email content begins ---
  From: Example Co Recruiting <jobs@example.com>
  Subject: Thank you for applying to Example Co

  Hi Ada,

  we have received your application for Senior Rust Engineer. Our team will review it and get back to you.

  Example Co Recruiting
  --- untrusted email content ends ---
classification:
  class: ack
  confidence: high
  score: 11
  evidence:
  - we have received your application
  - thank you for applying
matches:
- id: oa_ac9b7206
  company: Example Co
  title: Senior Rust Engineer
  status: applied
  score: 17
  reasons:
  - sender domain matches company
  - sender name contains company
  - subject contains company
  - body contains company
  - job title appears
ambiguous: false
applied: true
recorded:
  job_id: oa_ac9b7206
  event: ack
  status_before: applied
  status: acknowledged
  status_changed: true
  at: 2026-10-01T11:41:08Z
```

The echoed `email` block is the only place the message text appears, and it is always inside the delimiters. The history note for this event contains just `triage: classified ack (high confidence) from example.com`.

### 5. Chase and measure

```console
$ open-apply applied oa_806a425c --via email --at 2026-09-02
id: oa_806a425c
status: applied
company: Hooli
title: Ops Engineer
applied_at: 2026-09-02T00:00:00Z
via: email
applied_last_24h: 1
remaining_today: 24
hint: if nothing comes back, `open-apply followups` and `open-apply stale` will surface oa_806a425c

$ open-apply followups
days: 7
max_followups: 2
count: 1
jobs:
- id: oa_806a425c
  company: Hooli
  title: Ops Engineer
  status: applied
  applied_at: 2026-09-02T00:00:00Z
  days_since_activity: 29
  follow_ups_sent: 0
hint: 'send one short, polite note, then record it: open-apply event add <id> --type follow_up'

$ open-apply stale --days 21 --mark
days: 21
count: 1
marked: 1
jobs:
- id: oa_806a425c
  company: Hooli
  title: Ops Engineer
  applied_at: 2026-09-02T00:00:00Z
  days_waiting: 29
  marked: true

$ open-apply stats
jobs: 4
funnel:
  lead: 1
  saved: 1
  ready: 0
  applied: 0
  acknowledged: 1
  screening: 0
  interview: 0
  assessment: 0
  offer: 0
  accepted: 0
  rejected: 0
  ghosted: 1
  withdrawn: 0
  closed: 0
applied: 2
responded: 1
response_rate: 0.5
median_days_to_first_response: 0.0
ever_reached:
  acknowledged: 1
  screening: 0
  assessment: 0
  interview: 0
  offer: 0
by_source:
- key: manual
  applied: 2
  responded: 1
  response_rate: 0.5
by_via:
- key: ats
  applied: 1
  responded: 1
  response_rate: 1.0
- key: email
  applied: 1
  responded: 0
  response_rate: 0.0
```

Recording the same application twice is refused, and so is applying past a guardrail:

```console
$ open-apply applied oa_ac9b7206
error:
  code: conflict
  message: oa_ac9b7206 was already recorded as applied on 2026-10-01T11:41:08Z
  hint: use `open-apply event add` for follow-ups and responses, or `job update --applied-at` to correct the date
# exit code 7

$ open-apply applied oa_7a247ec1        # daily_application_cap: 1 in config.yaml
error:
  code: guardrail
  message: 'daily cap reached: 1 application(s) in the last 24 hours, cap is 1'
  hint: do not mass-apply; pick a better-fitting role, or pass --force if you have decided this is right
  detail:
    violations:
    - guardrail: daily_cap
      message: 'daily cap reached: 1 application(s) in the last 24 hours, cap is 1'
# exit code 6
```

---

## Command Reference

Global flags on every command: `--json` (JSON instead of YAML), `--quiet` (silence progress and warnings, which stderr shows only on a terminal anyway), `--home PATH` (data home; beats `$OPEN_APPLY_HOME`).

### Setup

| Command | Description |
|:---|:---|
| `open-apply init` | Create the data home, database, `profile.yaml` and `config.yaml` (safe to repeat) |
| `open-apply doctor` | Check home, config, database, schema, WAL mode, integrity and profile completeness; exit 4 on failure |
| `open-apply profile show` | Print the profile and what is missing |
| `open-apply profile path` | Print the path of `profile.yaml` |
| `open-apply profile set <key> <value>` | `name`, `email`, `phone`, `location`, `links.github`, `links.linkedin`, `links.site`, `cv_path`, `work_authorization`, `notice_period`, `salary_expectation`, `pronouns`, `answers.<question-key>` (empty value clears) |

### Discovery

| Command | Description |
|:---|:---|
| `open-apply source add <kind:ident>` | Watch a feed: `greenhouse:<board>`, `lever:<company>`, `ashby:<org>`, `remoteok[:<tag>]`, `weworkremotely[:<category>]`, `arbeitnow` |
| `open-apply source list` | List watched feeds |
| `open-apply source remove <kind:ident>` | Stop watching a feed |
| `open-apply search [--source S]... [--query Q] [--location L] [--remote] [--limit N] [--save]` | Query feeds (default: watched ones); `--save` stores results as `lead` jobs and skips duplicates |
| `open-apply job add <url> [--title --company --location --source --notes --no-fetch --allow-duplicate]` | Add a job by URL; ATS and JSON-LD details are read automatically |

### Jobs and materials

| Command | Description |
|:---|:---|
| `open-apply job list [--status --company --since --limit]` | List jobs (`--status` takes a comma-separated list) |
| `open-apply job show <id>` | One job with its wrapped description and full history |
| `open-apply job update <id> [--title --company --location --notes --source --applied-at --via]` | Correct fields; date and channel corrections leave a `note` event |
| `open-apply job rm <id> --yes` | Delete a job and its history (the workspace folder is left in place) |
| `open-apply next [--count N] [--force]` | The work queue: ready, then saved, then leads, with profile, answers and materials |
| `open-apply prepare <id>` | Create `workspaces/<id>/` and move the job to `ready` |
| `open-apply materials attach <id> --cv PATH [--cover PATH]` | Record the CV and cover letter to use |

### Record and track

| Command | Description |
|:---|:---|
| `open-apply applied <id> [--via linkedin\|ats\|email\|referral\|other] [--at DATE] [--note T] [--force]` | Record an application; enforces the guardrails |
| `open-apply status <id> <status> [--note T] [--at DATE]` | Set a status by hand (`applied` is refused, use `applied`) |
| `open-apply event add <id> --type ack\|screen\|interview\|assessment\|offer\|rejection\|follow_up\|note [--note T] [--at DATE]` | Append an event; statuses only move forward |
| `open-apply triage [--from ADDR] [--subject S] [--stdin] [--apply] [--job ID] [--at DATE]` | Classify an inbound email, match it to an application, optionally record it |
| `open-apply stale [--days 21] [--mark]` | List, or mark `ghosted`, applications nobody answered |
| `open-apply followups [--days 7]` | Applications due a polite follow-up |
| `open-apply stats [--since DATE]` | Funnel counts, response rate, median days to first response, by source and by channel |

### Data and agents

| Command | Description |
|:---|:---|
| `open-apply export [--format json\|csv\|md] [--out PATH]` | Export jobs; JSON includes the full history |
| `open-apply import <file>` | Import a JSON export (lossless) or a CSV with `url` and `title` columns |
| `open-apply agent-readme` | Print the agent manual (same text as `skills/open-apply/SKILL.md`) |
| `open-apply skill install [--dir PATH]` | Install the Claude Code skill to `<dir>/open-apply/SKILL.md` (default `~/.claude/skills`) |

Statuses: `lead`, `saved`, `ready`, `applied`, `acknowledged`, `screening`, `interview`, `assessment`, `offer`, `accepted`, `rejected`, `ghosted`, `withdrawn`, `closed`. Dates are `YYYY-MM-DD`, an RFC 3339 timestamp, `today` or `yesterday`, never in the future.

The top-level `--help`:

```text
Job search CLI built for LLM agents - discover postings, prepare materials, record every
application, track responses

Usage: open-apply [OPTIONS] <COMMAND>

Commands:
  init          Create the data home with a profile template and default config
  doctor        Check the home, database, schema and profile completeness
  profile       Show or change your profile (what the agent fills forms with)
  source        Manage watched public job feeds
  search        Search public job feeds (no login, no scraping)
  job           Add, list, show, update and remove jobs
  next          The agent work queue: next jobs to act on, with profile fields and saved answers
  prepare       Create the workspace for a job: job.md, profile.json, answers.json, cover-letter.md
  materials     Attach a CV and cover letter to a job
  applied       Record that you applied (enforces the daily cap and company cooldown)
  status        Set a job's status by hand
  event         Append an event to a job's history
  triage        Classify an inbound employer email and match it to an application
  stale         List or mark applications that never got a response
  followups     Applications due a polite follow-up
  stats         Funnel counts, response rate and response time
  export        Export jobs and history as json, csv or md
  import        Import a JSON export or a CSV of jobs
  agent-readme  Print the operating manual for an LLM agent
  skill         Install the Claude Code skill
  help          Print this message or the help of the given subcommand(s)

Options:
      --json         Emit JSON instead of YAML
      --quiet        Silence progress and warning messages (stderr shows them only on a terminal)
      --home <PATH>  Data home (default: $OPEN_APPLY_HOME or ~/.open-apply)
  -h, --help         Print help
  -V, --version      Print version
```

---

## Output Formats & Exit Codes

Commands print YAML on stdout by default. Pass `--json` when parsing with `jq`, Python or an agent tool loop:

```bash
open-apply --json next --count 3 | jq '.jobs[0].answers'
```

### Machine-readable error envelopes

Errors are written to stderr as `{error: {code, message, hint}}` (plus `detail` when there is structure to add), and the process exits with the matching code. Progress and warnings are printed only when stderr is a terminal, so captured stderr is always just the envelope.

| Exit Code | `code` | Meaning |
|:---|:---|:---|
| `0` | `ok` | Command completed successfully |
| `1` | `internal` | Unexpected failure; report it |
| `2` | `usage` | Bad flags or arguments |
| `3` | `not_found` | Unknown job id, uninitialized home, no matching application |
| `4` | `validation` | Input rejected (bad status, date, URL, profile key); also `doctor` with a failing check |
| `5` | `network` | Upstream unreachable, refusing, or returning something unexpected |
| `6` | `guardrail` | Daily cap, company cooldown or missing materials; `--force` overrides |
| `7` | `conflict` | Duplicate job, an application already recorded, or an ambiguous email match |

---

## Agent workflow loop

```
search / job add  ->  next  ->  prepare  ->  (tailor, apply in a browser)  ->  applied
       ^                                                                         |
       |                                                                         v
   stats, stale  <-  followups  <-  triage --apply (replies)  <-  wait for employers
```

1. **Discover**: `search --save` and `job add <url>` fill the queue.
2. **Pick**: `next --count 3` gives wrapped descriptions, profile fields, per-company answers and the guardrail state. Skip poor fits with `status <id> closed`.
3. **Prepare**: `prepare <id>`, read `job.md`, write `cover-letter.md`, optionally `materials attach`.
4. **Apply**: in a browser, with the user's go-ahead for each submission. Stop and ask on anything the profile cannot answer.
5. **Record**: `applied <id> --via ...` only after the application was really submitted.
6. **Triage**: pipe each reply into `triage`, check the dry run, then `--apply`.
7. **Follow up**: `followups`, `stale --mark`, `stats`.

The full rules for an agent are in [`skills/open-apply/SKILL.md`](skills/open-apply/SKILL.md); the reasoning is in [`docs/agent-guide.md`](docs/agent-guide.md), the storage layout in [`docs/schema.md`](docs/schema.md). Reading replies is up to your mail tool (for example [Gmail-Cli](https://github.com/SpaceCorps/Gmail-Cli)); open-apply only needs the sender, subject and body.

---

## Configuration & Environment Variables

`config.yaml` (created by `init`):

| Key | Default | Description |
|:---|:---|:---|
| `daily_application_cap` | `25` | Most applications in any rolling 24 hours; `0` disables |
| `company_cooldown_days` | `90` | Days between applications to the same company; `0` disables |
| `require_materials` | `false` | When true, a CV file must resolve from the job or the profile |
| `max_followups` | `2` | Follow-ups `followups` suggests per application |
| `http_timeout_secs` | `20` | Timeout per HTTP request |
| `request_delay_ms` | `500` | Minimum gap between requests to one host |

| Variable | Description | Default |
|:---|:---|:---|
| `OPEN_APPLY_HOME` | Data home | `~/.open-apply` |
| `OPEN_APPLY_NOW` | Pin the clock (RFC 3339), for tests | real time |
| `OPEN_APPLY_REQUEST_DELAY_MS` | Override `request_delay_ms` | from `config.yaml` |
| `OPEN_APPLY_GREENHOUSE_URL`, `OPEN_APPLY_LEVER_URL`, `OPEN_APPLY_ASHBY_URL`, `OPEN_APPLY_REMOTEOK_URL`, `OPEN_APPLY_WWR_URL`, `OPEN_APPLY_ARBEITNOW_URL` | Override a feed's base URL (how the test suite points at its mock) | the public endpoints |

---

## Design notes

**What it will not do.** open-apply does not scrape LinkedIn. It does not bypass CAPTCHAs or bot detection. It does not store site passwords. It does not submit forms itself. The agent, or you, applies in a browser; open-apply supplies the data (`next`, `prepare`) and records the result (`applied`), so it stays the system of record wherever the application was actually made: LinkedIn Easy Apply, a company ATS, email or a referral. LinkedIn, Indeed, Glassdoor and similar URLs are canonicalized and tracked, and never fetched. A 401, 403 or 429 from any endpoint is reported and not retried around.

**Why guardrails.** Low-quality mass-applying hurts the applicant: generic applications sent in volume tend to get fewer answers and use up goodwill with the companies you most want. The defaults (25 per rolling 24 hours, 90 days between roles at one company) slow you down on purpose. They are checked inside the transaction that records the application, `--force` overrides them, and the override is written into that application's history.

**Untrusted input.** Job descriptions and emails are written by strangers and can carry instructions aimed at an agent. Whenever they are echoed they are wrapped in `--- untrusted job content begins ---` / `--- untrusted job content ends ---` (and the `email` equivalents), with any delimiter-looking text inside neutralized so it cannot end the block early. Triage classifies with fixed phrase lists and never executes, follows or stores anything from the message: history notes contain only the class, the confidence and the sender's domain. Titles, companies and locations in listings are third-party text too.

**History is append-only.** `events` has a trigger that rejects updates. Every status change appends a row (type, note, when it happened, source, when it was recorded). Only `job rm` removes history, together with its job.

**Dedupe.** A posting is a duplicate when its canonical URL is already stored (tracking parameters, fragments, `www.` and trailing slashes removed; LinkedIn `/jobs/view/<id>` variants and the Greenhouse, Lever and Ashby URL shapes collapsed) or when the same normalized company, title and location is. Job ids are `oa_` plus 8 hex characters of the SHA-256 of the canonical URL.

**Response rate.** A response is any employer answer: acknowledgement, screen, interview, assessment, offer or rejection. Response time runs from `applied_at` to the first such event on or after it.

**Tested offline.** `cargo test --locked` opens no connection to the internet. Each feed sits behind a small trait with a pure `parse` function; the fixtures in `tests/fixtures/` are written to the documented response shapes (they are not recordings), and the integration tests run the real binary against an in-process mock of the endpoints. During development each feed parser and each single-posting ATS endpoint was also checked once against the live service and parsed correctly; that was a manual check and is not part of the suite.

---

## Known limits and roadmap

What is deliberately simple today:

- Triage is rule-based and English-only. It reports its confidence and will not record a low-confidence result on its own.
- Mail is not fetched by open-apply; the agent passes sender, subject and body in.
- The daily cap is a rolling 24-hour window on the application timestamp, not a calendar day, so it does not depend on a timezone.
- `job add` reads Greenhouse, Lever and Ashby through their job-page URLs. A Greenhouse posting on a company's own domain (`?gh_jid=`) is read through its JSON-LD if the page has any, not through the Greenhouse API.
- Ashby has no single-posting endpoint, so a lookup fetches the whole board.
- There is no `config` command yet: guardrails are edited in `config.yaml`.
- The release workflow builds five targets, matching the Gmail-Cli workflow, and has not been run.

Possible next steps: more public feeds (Workable, Recruitee, SmartRecruiters, Personio), a `config set` command, triage in more languages, a digest command for scheduled runs, and importing from other trackers.

---

## Contributing & License

See [AGENTS.md](AGENTS.md) for the layout, invariants and how to add a feed. Maintained by [SpaceCorps](https://github.com/SpaceCorps). Released under the [MIT License](LICENSE).
