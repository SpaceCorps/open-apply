//! Small zero-dependency helpers: SHA-256, UTC time handling, HTML-to-text, CSV, text normalization.

use crate::error::{Error, Result};

// ---------------------------------------------------------------------------------------------
// SHA-256 (FIPS 180-4). Used only to derive short stable job ids from canonical URLs.
// ---------------------------------------------------------------------------------------------

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98,
    0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
    0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8,
    0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819,
    0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
    0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
    0xc67178f2,
];

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] =
        [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, v) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(v);
        }
    }
    let mut out = [0u8; 32];
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ---------------------------------------------------------------------------------------------
// UTC time. Timestamps are stored as `YYYY-MM-DDTHH:MM:SSZ` so string order is time order.
// ---------------------------------------------------------------------------------------------

pub const DAY: i64 = 86_400;

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

pub fn format_rfc3339(secs: i64) -> String {
    let (y, m, d) = civil_from_days(secs.div_euclid(DAY));
    let rem = secs.rem_euclid(DAY);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

fn valid_date(y: i64, m: u32, d: u32) -> bool {
    if !(1..=12).contains(&m) || d < 1 {
        return false;
    }
    let (y2, m2, d2) = civil_from_days(days_from_civil(y, m, d));
    (y2, m2, d2) == (y, m, d)
}

/// Accepts `YYYY-MM-DD`, `YYYY-MM-DDTHH:MM[:SS[.fff]]` with `Z` or `+HH:MM` (or none, read as UTC),
/// and `YYYY-MM-DD HH:MM[:SS]`.
pub fn parse_timestamp(input: &str) -> Option<i64> {
    let s = input.trim();
    if s.len() < 10 || !s.is_char_boundary(10) {
        return None;
    }
    let (date, rest) = s.split_at(10);
    let mut parts = date.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || date.len() != 10 || !valid_date(y, m, d) {
        return None;
    }
    let base = days_from_civil(y, m, d) * DAY;
    if rest.is_empty() {
        return Some(base);
    }
    let rest = rest.strip_prefix('T').or_else(|| rest.strip_prefix(' '))?;
    // Split the zone off the time part.
    let (time, offset) = if let Some(t) = rest.strip_suffix('Z').or_else(|| rest.strip_suffix('z')) {
        (t, 0)
    } else if let Some(idx) = rest.rfind(['+', '-']).filter(|&i| i >= 5) {
        let (t, z) = rest.split_at(idx);
        let sign = if z.starts_with('-') { -1 } else { 1 };
        let z = &z[1..];
        let (zh, zm) = z.split_once(':').unwrap_or_else(|| if z.len() == 4 { z.split_at(2) } else { (z, "0") });
        let off: i64 = zh.parse::<i64>().ok()? * 3600 + zm.parse::<i64>().ok()? * 60;
        (t, sign * off)
    } else {
        (rest, 0)
    };
    let time = time.split('.').next()?;
    let mut t = time.split(':');
    let hh: i64 = t.next()?.parse().ok()?;
    let mm: i64 = t.next()?.parse().ok()?;
    let ss: i64 = t.next().map(str::parse).transpose().ok()?.unwrap_or(0);
    if hh > 23 || mm > 59 || ss > 59 || t.next().is_some() {
        return None;
    }
    Some(base + hh * 3600 + mm * 60 + ss - offset)
}

/// RFC 2822 dates as used in RSS `pubDate`: `Mon, 22 Sep 2026 10:00:00 +0000`.
pub fn parse_rfc2822(input: &str) -> Option<i64> {
    let s = input.trim();
    let s = s.split_once(',').map(|(_, r)| r).unwrap_or(s).trim();
    let mut it = s.split_whitespace();
    let d: u32 = it.next()?.parse().ok()?;
    let mon = it.next()?.to_ascii_lowercase();
    let months = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let m = months.iter().position(|x| mon.starts_with(x))? as u32 + 1;
    let y: i64 = it.next()?.parse().ok()?;
    let time = it.next().unwrap_or("00:00:00");
    let zone = it.next().unwrap_or("+0000");
    if !valid_date(y, m, d) {
        return None;
    }
    let mut t = time.split(':');
    let hh: i64 = t.next()?.parse().ok()?;
    let mm: i64 = t.next()?.parse().ok()?;
    let ss: i64 = t.next().map(str::parse).transpose().ok()?.unwrap_or(0);
    let off = if zone.len() == 5 && (zone.starts_with('+') || zone.starts_with('-')) {
        let sign = if zone.starts_with('-') { -1 } else { 1 };
        sign * (zone[1..3].parse::<i64>().ok()? * 3600 + zone[3..5].parse::<i64>().ok()? * 60)
    } else {
        0
    };
    Some(days_from_civil(y, m, d) * DAY + hh * 3600 + mm * 60 + ss - off)
}

/// The current time. `OPEN_APPLY_NOW` (an RFC 3339 timestamp) pins it so tests are deterministic.
pub fn now_secs() -> i64 {
    if let Ok(v) = std::env::var("OPEN_APPLY_NOW")
        && let Some(t) = parse_timestamp(&v)
    {
        return t;
    }
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn now_rfc3339() -> String {
    format_rfc3339(now_secs())
}

/// Resolves a user-supplied `--at` value. Absent means now. Dates in the future are rejected
/// because every timestamp here records something that already happened.
pub fn resolve_at(at: Option<&str>) -> Result<String> {
    let now = now_secs();
    let Some(raw) = at else { return Ok(format_rfc3339(now)) };
    let secs = match raw.trim().to_ascii_lowercase().as_str() {
        "now" | "today" => now,
        "yesterday" => now - DAY,
        _ => parse_timestamp(raw).ok_or_else(|| {
            Error::validation(format!("cannot read date '{raw}'"))
                .hint("use YYYY-MM-DD, YYYY-MM-DDTHH:MM:SSZ, 'today' or 'yesterday'")
        })?,
    };
    if secs > now + 300 {
        return Err(Error::validation(format!("date '{raw}' is in the future"))
            .hint("--at records when something happened, so it cannot be later than now"));
    }
    Ok(format_rfc3339(secs))
}

/// Same parsing as `resolve_at` but for filters (`--since`), where the future is harmless.
pub fn parse_since(raw: &str) -> Result<String> {
    parse_timestamp(raw)
        .map(format_rfc3339)
        .ok_or_else(|| Error::validation(format!("cannot read date '{raw}'")).hint("use YYYY-MM-DD"))
}

/// Whole days between two RFC 3339 timestamps, as a float. `None` when either fails to parse.
pub fn days_between(from: &str, to: &str) -> Option<f64> {
    Some((parse_timestamp(to)? - parse_timestamp(from)?) as f64 / DAY as f64)
}

// ---------------------------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------------------------

pub fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let decoded = after.find(';').filter(|&e| e <= 10).and_then(|e| {
            let name = &after[..e];
            let ch = if let Some(num) = name.strip_prefix('#') {
                let code = match num.strip_prefix(['x', 'X']) {
                    Some(h) => u32::from_str_radix(h, 16).ok(),
                    None => num.parse().ok(),
                };
                code.and_then(char::from_u32)
            } else {
                match name {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "nbsp" => Some(' '),
                    "ndash" => Some('-'),
                    "mdash" => Some('-'),
                    "hellip" => Some('…'),
                    "rsquo" | "lsquo" => Some('\''),
                    "ldquo" | "rdquo" => Some('"'),
                    "bull" | "middot" => Some('•'),
                    "copy" => Some('©'),
                    "reg" => Some('®'),
                    "trade" => Some('™'),
                    "euro" => Some('€'),
                    _ => None,
                }
            };
            ch.map(|c| (c, e))
        });
        match decoded {
            Some((c, e)) => {
                out.push(c);
                rest = &after[e + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Strips markup and keeps a readable plain-text shape: paragraphs and list items become lines.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let after = &rest[start..];
        if after.starts_with("<!--") {
            rest = after.find("-->").map(|e| &after[e + 3..]).unwrap_or("");
            continue;
        }
        let next = after[1..].chars().next().unwrap_or(' ');
        if !(next.is_ascii_alphabetic() || next == '/' || next == '!' || next == '?') {
            out.push('<');
            rest = &after[1..];
            continue;
        }
        let Some(end) = after.find('>') else {
            out.push_str(after);
            rest = "";
            break;
        };
        let tag = &after[1..end];
        let closing = tag.starts_with('/');
        let name = tag
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if (name == "script" || name == "style") && !closing {
            let lower = after.to_ascii_lowercase();
            let close = format!("</{name}");
            rest = match lower.find(&close) {
                Some(c) => after[c..].find('>').map(|g| &after[c + g + 1..]).unwrap_or(""),
                None => "",
            };
            continue;
        }
        match name.as_str() {
            "br" => out.push('\n'),
            "li" if !closing => out.push_str("\n- "),
            "li" => {}
            "p" | "div" | "tr" | "ul" | "ol" | "table" | "section" | "article" | "header" | "footer" | "blockquote"
            | "pre" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => out.push('\n'),
            "td" | "th" => out.push(' '),
            _ => {}
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    collapse_text(&decode_entities(&out))
}

/// Trims lines, collapses runs of spaces and limits blank lines to one in a row.
pub fn collapse_text(s: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut blank = true; // swallow leading blanks
    for raw in s.replace("\r\n", "\n").replace('\r', "\n").split('\n') {
        let line = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() || line == "-" {
            if !blank {
                lines.push(String::new());
                blank = true;
            }
        } else {
            lines.push(line);
            blank = false;
        }
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines.join("\n")
}

/// Truncates on a char boundary, appending an ellipsis marker when something was cut.
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{}...", cut.trim_end())
}

/// Lowercase alphanumeric tokens.
pub fn tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()).map(str::to_lowercase).collect()
}

/// "acme-labs" -> "Acme Labs". Used when an ATS gives only a board token, not a company name.
pub fn prettify_slug(slug: &str) -> String {
    slug.split(['-', '_', '.'])
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut c = p.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

const COMPANY_SUFFIXES: &[&str] = &[
    "inc",
    "incorporated",
    "llc",
    "ltd",
    "limited",
    "gmbh",
    "oy",
    "ab",
    "as",
    "corp",
    "corporation",
    "co",
    "company",
    "plc",
    "sa",
    "bv",
    "ag",
    "sarl",
    "srl",
    "pte",
    "oyj",
    "kg",
    "nv",
];

/// Normalized company name for matching: lowercase alphanumeric words with legal suffixes removed.
pub fn company_key(company: &str) -> String {
    let mut toks = tokens(company);
    while toks.len() > 1 && toks.last().is_some_and(|t| COMPANY_SUFFIXES.contains(&t.as_str())) {
        toks.pop();
    }
    toks.join(" ")
}

/// Key used to spot the same posting arriving from two places: company, title and location.
pub fn dedupe_key(company: &str, title: &str, location: &str) -> String {
    let mut loc = tokens(location);
    loc.sort();
    loc.dedup();
    format!("{}|{}|{}", company_key(company), tokens(title).join(" "), loc.join(" "))
}

// ---------------------------------------------------------------------------------------------
// CSV (RFC 4180 subset)
// ---------------------------------------------------------------------------------------------

pub fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

pub fn csv_row(fields: &[String]) -> String {
    fields.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(",")
}

pub fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
        } else {
            match c {
                '"' if field.is_empty() => quoted = true,
                ',' => row.push(std::mem::take(&mut field)),
                '\r' => {}
                '\n' => {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                }
                _ => field.push(c),
            }
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows.into_iter().filter(|r| !(r.len() == 1 && r[0].is_empty())).collect()
}

pub fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = values.len() / 2;
    Some(if values.len() % 2 == 1 { values[mid] } else { (values[mid - 1] + values[mid]) / 2.0 })
}

pub fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

pub fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_known_vectors() {
        assert_eq!(hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        let long = "a".repeat(1000);
        assert_eq!(hex(&sha256(long.as_bytes())), "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3");
    }

    #[test]
    fn civil_round_trip() {
        for secs in [0, 86_399, 951_782_400, 1_790_000_000, 4_102_444_800] {
            assert_eq!(parse_timestamp(&format_rfc3339(secs)), Some(secs));
        }
        assert_eq!(format_rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_rfc3339(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn parses_dates_and_offsets() {
        assert_eq!(parse_timestamp("2026-09-01"), parse_timestamp("2026-09-01T00:00:00Z"));
        assert_eq!(parse_timestamp("2026-09-01T12:00:00+02:00"), parse_timestamp("2026-09-01T10:00:00Z"));
        assert_eq!(parse_timestamp("2026-09-01 10:30"), parse_timestamp("2026-09-01T10:30:00Z"));
        assert_eq!(parse_timestamp("2026-09-01T10:00:00.123Z"), parse_timestamp("2026-09-01T10:00:00Z"));
        assert_eq!(parse_timestamp("2026-02-30"), None);
        assert_eq!(parse_timestamp("yesterday"), None);
        assert_eq!(parse_timestamp("2026-13-01"), None);
    }

    #[test]
    fn parses_rfc2822() {
        assert_eq!(parse_rfc2822("Mon, 22 Sep 2026 10:00:00 +0000"), parse_timestamp("2026-09-22T10:00:00Z"));
        assert_eq!(parse_rfc2822("Tue, 23 Sep 2026 10:00:00 +0200"), parse_timestamp("2026-09-23T08:00:00Z"));
        assert_eq!(parse_rfc2822("nonsense"), None);
    }

    #[test]
    fn html_to_text_keeps_structure() {
        let html = "<style>p{}</style><h2>About</h2><p>We build&nbsp;things &amp; more.</p><ul><li>Rust</li><li>SQL</li></ul><script>x()</script>";
        let text = html_to_text(html);
        assert_eq!(text, "About\n\nWe build things & more.\n\n- Rust\n- SQL");
    }

    #[test]
    fn html_to_text_tolerates_stray_angle_brackets() {
        assert_eq!(html_to_text("3 < 5 and <b>bold</b>"), "3 < 5 and bold");
    }

    #[test]
    fn entities_decode() {
        assert_eq!(
            decode_entities("&lt;p&gt;a &amp; b&lt;/p&gt; &#39;x&#39; &#x41; &bogus; & end"),
            "<p>a & b</p> 'x' A &bogus; & end"
        );
    }

    #[test]
    fn company_and_dedupe_keys() {
        assert_eq!(company_key("Acme, Inc."), "acme");
        assert_eq!(company_key("Acme Labs GmbH"), "acme labs");
        assert_eq!(company_key("Co"), "co");
        assert_eq!(
            dedupe_key("Acme Inc", "Senior  Rust Engineer", "Berlin, Germany"),
            dedupe_key("ACME", "senior rust engineer", "Germany Berlin")
        );
        assert_ne!(dedupe_key("Acme", "Engineer", "Berlin"), dedupe_key("Acme", "Engineer", "Paris"));
    }

    #[test]
    fn csv_round_trip() {
        let rows = vec![vec!["a".to_string(), "b,c".to_string(), "say \"hi\"".to_string(), "two\nlines".to_string()]];
        let text = csv_row(&rows[0]);
        assert_eq!(parse_csv(&format!("{text}\n")), rows);
        assert_eq!(parse_csv("x,y\r\n1,2\r\n"), vec![vec!["x", "y"], vec!["1", "2"]]);
    }

    #[test]
    fn median_odd_even() {
        assert_eq!(median(&mut [3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&mut [4.0, 1.0, 2.0, 3.0]), Some(2.5));
        assert_eq!(median(&mut []), None);
    }

    #[test]
    fn prettify() {
        assert_eq!(prettify_slug("acme-labs"), "Acme Labs");
        assert_eq!(prettify_slug("stripe"), "Stripe");
    }

    #[test]
    fn truncates_on_char_boundary() {
        assert_eq!(truncate_chars("ääääää", 3), "äää...");
        assert_eq!(truncate_chars("abc", 3), "abc");
    }
}
