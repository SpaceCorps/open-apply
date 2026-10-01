# Schema reference

Everything open-apply stores lives under the data home: `$OPEN_APPLY_HOME` or `~/.open-apply`.

```
open-apply.db        SQLite, WAL mode
profile.yaml         who you are and reusable answers
config.yaml          guardrails and HTTP settings
workspaces/<job-id>/ job.md, profile.json, answers.json, cover-letter.md
```

## SQLite

The database runs in WAL mode with foreign keys on. The schema version is `PRAGMA user_version`; each entry in
the migration list in `src/db.rs` is applied once, in a transaction, and a database newer than the binary is
refused. Current version: 1.

### `jobs`

| Column | Type | Notes |
|:---|:---|:---|
| `id` | TEXT PK | `oa_` + 8 hex characters from SHA-256 of `canonical_url` |
| `url` | TEXT | as the user or feed gave it |
| `canonical_url` | TEXT UNIQUE | tracking parameters, fragment, `www.` and trailing slash removed; LinkedIn, Indeed and ATS variants collapsed |
| `title`, `company`, `location` | TEXT | third-party text |
| `company_key` | TEXT | normalized company (lowercase, legal suffixes dropped), used for cooldown and triage |
| `remote` | INTEGER | NULL unknown, 0 or 1 |
| `source` | TEXT | `manual`, `import`, or the feed label such as `greenhouse:acme` |
| `ats` | TEXT | `greenhouse`, `lever`, `ashby`, or a tracking-only board name; NULL otherwise |
| `description` | TEXT | plain text, third-party |
| `notes` | TEXT | yours |
| `status` | TEXT | see below |
| `tracking_only` | INTEGER | 1 for boards that are never fetched |
| `dedupe_key` | TEXT | normalized `company|title|location` |
| `cv_path`, `cover_path` | TEXT | attached files (absolute paths) |
| `posted_at` | TEXT | from the feed, RFC 3339 |
| `applied_at`, `applied_via` | TEXT | set by `applied`; `applied_via` is one of linkedin, ats, email, referral, other |
| `created_at`, `updated_at` | TEXT | RFC 3339 UTC, when the row was written |

Indexes: `status`, `company_key`, `dedupe_key`, `applied_at`.

### `events`

Append-only history. A trigger (`events_append_only`) aborts any UPDATE. Rows leave only when `job rm`
deletes their job.

| Column | Type | Notes |
|:---|:---|:---|
| `id` | INTEGER PK AUTOINCREMENT | |
| `job_id` | TEXT | references `jobs(id)` |
| `type` | TEXT | `created`, `applied`, `status`, `ack`, `screen`, `interview`, `assessment`, `offer`, `rejection`, `follow_up`, `note` |
| `status` | TEXT | the status the job had after this event, when it changed |
| `note` | TEXT | free text; triage writes only class, confidence and sender domain |
| `at` | TEXT | when it happened (what `--at` sets) |
| `source` | TEXT | `cli`, `search`, `triage`, `stale`, `import` |
| `recorded_at` | TEXT | when the row was written |

`job show` lists the `created` event first and the rest ordered by `at`.

### `sources`

`kind`, `ident`, `added_at`, primary key `(kind, ident)`. `ident` is empty for feeds without one.

## Statuses

`lead` (found by search), `saved` (added by hand), `ready` (workspace prepared), `applied`, `acknowledged`,
`screening`, `interview`, `assessment`, `offer`, `accepted`, `rejected`, `ghosted`, `withdrawn`, `closed`.

Inbound events advance a status along this rank and never backwards: lead < saved < ready < applied = ghosted
< acknowledged < screening < assessment < interview < offer < accepted. `rejection` moves any job that is
not already accepted, rejected, withdrawn or closed to `rejected`. `status <id> <status>` sets any status
except `applied` and writes an event of the matching type.

## Event types and what they do

| Type | Written by | Moves status to |
|:---|:---|:---|
| `created` | job add, search --save, import | |
| `applied` | `applied` | applied |
| `status` | `status`, `prepare`, `stale --mark` | the new status |
| `ack` | `event add`, triage | acknowledged |
| `screen` | `event add`, triage | screening |
| `assessment` | `event add`, triage | assessment |
| `interview` | `event add`, triage | interview |
| `offer` | `event add`, triage | offer |
| `rejection` | `event add`, triage | rejected |
| `follow_up` | `event add` | |
| `note` | `event add`, `job update --applied-at/--via` | |

`ack`, `screen`, `assessment`, `interview`, `offer` and `rejection` count as an employer response in `stats`.

## `config.yaml`

```yaml
daily_application_cap: 25     # 0 disables
company_cooldown_days: 90     # 0 disables
require_materials: false
max_followups: 2
http_timeout_secs: 20
request_delay_ms: 500
```

Missing keys take these defaults.

## `profile.yaml`

```yaml
name: ''
email: ''
phone: ''
location: ''
links:
  github: ''
  linkedin: ''
  site: ''
cv_path: ''
work_authorization: ''
notice_period: ''
salary_expectation: ''
pronouns: ''
answers: {}        # question-key: text; {company}, {title}, {location} are filled in per job
```

`name` and `email` are required for `doctor` to pass. An answer key `why_us@acme` applies to the company whose
normalized name is `acme` only.

## JSON export (`export --format json`)

```json
{
  "format": "open-apply-export",
  "version": 1,
  "exported_at": "2026-10-01T12:00:00Z",
  "jobs": [
    {
      "id": "oa_ac9b7206",
      "url": "...", "canonical_url": "...", "title": "...", "company": "...", "location": "...",
      "remote": null, "source": "manual", "ats": null, "description": "", "notes": "",
      "status": "acknowledged", "tracking_only": false, "cv_path": null, "cover_path": null,
      "posted_at": null, "applied_at": "...", "applied_via": "ats",
      "created_at": "...", "updated_at": "...",
      "events": [
        {"type": "applied", "status": "applied", "note": null, "at": "...", "source": "cli", "recorded_at": "..."}
      ]
    }
  ]
}
```

`import` skips a job whose id or canonical URL already exists, so importing the same file twice is safe.

## CSV

Export columns: `id, status, title, company, location, source, url, applied_via, applied_at, posted_at,
created_at, updated_at, notes`. Import matches header names case-insensitively, requires `url` and `title`,
and treats a row with `applied_at` and no `status` as applied. Imported CSV rows get a `created` event and,
when applied, an `applied` event; blank rows are ignored.

## Error envelope

```yaml
error:
  code: guardrail        # usage | not_found | validation | network | guardrail | conflict | internal
  message: daily cap reached: 25 application(s) in the last 24 hours, cap is 25
  hint: stop for today; ...
  detail: {}             # optional, structured
```

| Exit | `code` |
|:---|:---|
| 0 | ok |
| 1 | internal |
| 2 | usage |
| 3 | not_found |
| 4 | validation |
| 5 | network |
| 6 | guardrail |
| 7 | conflict |
