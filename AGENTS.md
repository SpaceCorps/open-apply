# AGENTS.md

Notes for whoever extends this next.

`open-apply` is a native Rust 2024 CLI that lets an LLM agent (or a person) run a job search end to end:
find postings in public feeds, prepare materials per job, record every application, and track whether and
when employers respond. It is the system of record. It never submits an application itself.

For the manual the *agent* reads, run `open-apply agent-readme`. That text is `skills/open-apply/SKILL.md`,
embedded with `include_str!` in `src/readme.rs`, so there is one source of truth. This file is for the human
editing the source.

## Developer Commands

```bash
cargo build --release              # target/release/open-apply
cargo test --locked                # unit tests + tests/*.rs against an in-process mock, no network
cargo clippy --all-targets --locked -- -D warnings
cargo fmt --check
cargo install --path . --locked    # put it on PATH
```

Use a throwaway home when trying things by hand so you never touch your real search:

```bash
export OPEN_APPLY_HOME=$(mktemp -d)/home
open-apply init
```

| Variable | Effect |
| --- | --- |
| `OPEN_APPLY_HOME` | Data home (default `~/.open-apply`); `--home PATH` wins over it |
| `OPEN_APPLY_NOW` | RFC 3339 timestamp that pins the clock (tests use it; stale, follow-up and cap math depend on "now") |
| `OPEN_APPLY_REQUEST_DELAY_MS` | Overrides the per-host politeness delay from `config.yaml` (tests set `0`) |
| `OPEN_APPLY_GREENHOUSE_URL` | Base URL for the Greenhouse boards API |
| `OPEN_APPLY_LEVER_URL` | Base URL for the Lever postings API |
| `OPEN_APPLY_ASHBY_URL` | Base URL for the Ashby job board API |
| `OPEN_APPLY_REMOTEOK_URL` | Base URL for RemoteOK |
| `OPEN_APPLY_WWR_URL` | Base URL for We Work Remotely |
| `OPEN_APPLY_ARBEITNOW_URL` | Base URL for Arbeitnow |

## Layout

```
src/
  main.rs            arg pre-scan for --json, clap errors -> structured usage envelopes
  cli.rs             command tree (clap derive); help text lives here
  commands/
    mod.rs           dispatch, Out type, shared helpers (job summary, answers, materials)
    init.rs doctor.rs profile.rs source.rs search.rs job.rs next.rs prepare.rs applied.rs
    status.rs        `status` and `event add`
    triage.rs stale.rs followups.rs stats.rs export_import.rs skill.rs
  sources/
    mod.rs           JobSource trait, Posting, SourceSpec, Http (UA, timeouts, politeness), Query filter
    greenhouse.rs lever.rs ashby.rs remoteok.rs weworkremotely.rs arbeitnow.rs
    jsonld.rs        schema.org JobPosting extraction from HTML
  db.rs              SQLite schema + user_version migrations, job/event queries, transactions
  model.rs           Status, EventType, Via, Job, Event, status advance rules
  guardrails.rs      daily cap / cooldown / materials evaluation
  triage.rs          email classifier and job matcher (pure)
  stats.rs           funnel statistics (pure)
  export.rs          json/csv/md export and json/csv import
  url.rs             URL parsing, canonicalization, site classification
  config.rs          Home, config.yaml, profile.yaml
  output.rs          YAML/JSON rendering, error envelope, untrusted-content delimiters
  error.rs           ErrorCode (exit codes 0-7) and Error
  util.rs            SHA-256, UTC time, HTML to text, CSV, normalization
  readme.rs          embeds SKILL.md, exit code table
skills/open-apply/SKILL.md   the agent operating manual (also what `agent-readme` prints)
tests/
  common/mod.rs     isolated home, command builder that cannot reach the internet, mock HTTP server
  fixtures/         response bodies in the documented shapes of each public endpoint
  lifecycle.rs exit_codes.rs sources.rs guardrails.rs triage.rs tracking.rs agent.rs
docs/               agent guide and schema reference
site/               landing page and docs site (Vite+)
```

## Architectural Principles & Invariants

**1. It never submits anything.** There is no code path that posts an application form, logs in anywhere,
stores a site password, solves a CAPTCHA or works around bot detection. The agent or the human applies in a
browser; `applied` records that it happened. Do not add a "submit" command.

**2. No scraping of job boards.** LinkedIn, Indeed, Glassdoor and similar URLs are canonicalized and tracked
(`Site::TrackingOnly` in `src/url.rs`) and are never fetched. Only documented, unauthenticated endpoints are
queried (Greenhouse, Lever, Ashby, RemoteOK, We Work Remotely, Arbeitnow), plus a single GET of a URL the
user hands to `job add`. A 401, 403 or 429 is reported, never retried around.

**3. Third-party text is untrusted.** Job descriptions and emails are wrapped by `output::wrap_job` and
`output::wrap_email` whenever they are echoed, and any delimiter-looking text inside is neutralized so a
posting cannot close its own block. Triage never stores or echoes raw email text outside those delimiters:
history notes contain only the class, confidence and sender domain. Nothing in a posting or an email is ever
interpreted as a command.

**4. Events are append-only.** `events` has a trigger that aborts UPDATE. Status changes always go through
`db::change_status`, `db::record_event` or `db::write_applied`, which append a row in the same transaction.
Only `job rm` removes history, and only together with its job.

**5. Guardrails are enforced where the write happens.** `applied` checks them inside the same immediate
transaction that records the application, so two concurrent runs cannot both slip under the cap. `--force`
overrides them and the override is written into the event note.

**6. Deterministic offline testing.** `cargo test --locked` opens no connection to the internet. Endpoint
base URLs are overridable, the test harness points them at a dead local port by default, and the mock server
in `tests/common/mod.rs` serves fixtures. The fixtures are written to the documented response shapes; they
are not recordings. When an upstream changes shape, update the fixture and the parser together.

**7. stdout is data, stderr is the envelope.** Commands print YAML (or JSON with `--json`) on stdout. Errors
are `{error: {code, message, hint}}` on stderr with the exit code from `src/error.rs`. Progress and warning
chatter is written to stderr only when stderr is a terminal, so captured stderr always parses.

**8. Stable exit codes.** 0 ok, 1 internal, 2 usage, 3 not found, 4 validation, 5 network, 6 guardrail,
7 conflict. Agents branch on these; do not renumber them.

## Adding a feed

1. Add `src/sources/<name>.rs` with a type implementing `JobSource` (`label`, `endpoint`, `parse`) and pure
   parse functions. Give the base URL an `OPEN_APPLY_<NAME>_URL` override.
2. Register it in `SourceSpec` (`src/sources/mod.rs`).
3. Add a fixture under `tests/fixtures/`, unit tests next to the parser, and a route in `Mock::feeds()`.
4. Document it in README, the skill, and `docs/agent-guide.md`.

## Releasing

CI (`.github/workflows/ci.yml`) runs formatting, Clippy and tests on Linux, macOS and Windows. A tag `vX.Y.Z`
starts `.github/workflows/release.yml`, which builds five targets and attaches them to a GitHub release.
`version` in `Cargo.toml` must match the tag; the workflow checks it.

```bash
git tag v0.1.0 && git push origin v0.1.0
```

## The site

`site/` is the landing page and command reference, built with Vite+ (`vp`). It is a static page served from
`https://spacecorps.github.io/open-apply/` (Vite `base` is `/open-apply/`).

```bash
cd site
vp install
vp dev                 # http://localhost:5173/open-apply/
vp check               # format, lint, types
vp test                # vitest
vp build               # site/dist
node scripts/gen-commands.mjs   # regenerate src/generated/commands.json from the built binary
```

The command reference is generated from `open-apply --help`; after changing any help text, run
`cargo build --release` and the generator, and commit the JSON. `tests/docs.rs` fails when the committed JSON
drifts from the real help output. `.github/workflows/pages.yml` builds and deploys it.
