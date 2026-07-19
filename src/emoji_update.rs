//! Download Unicode emoji-test + emojilib keywords and write `emojis.json`.
//!
//! Used by `update-emojis` (dev: write into `assets/`) and by Settings
//! (user: write into XDG data dir so the picker picks it up without rebuild).

use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const EMOJI_TEST_URL: &str = "https://unicode.org/Public/emoji/latest/emoji-test.txt";
const EMOJILIB_URL: &str =
    "https://raw.githubusercontent.com/muan/emojilib/main/dist/emoji-en-US.json";

const SKIN_TONES: &[char] = &[
    '\u{1F3FB}',
    '\u{1F3FC}',
    '\u{1F3FD}',
    '\u{1F3FE}',
    '\u{1F3FF}',
];
const VS16: char = '\u{FE0F}';

#[derive(Debug, Serialize)]
struct JsonEmoji {
    codes: String,
    char: String,
    name: String,
    category: String,
    group: String,
    subgroup: String,
    keywords: String,
}

/// Result of a successful catalogue refresh.
#[derive(Debug, Clone)]
pub struct UpdateReport {
    pub version: String,
    pub count: usize,
    pub with_keywords: usize,
    pub json_path: PathBuf,
}

/// Fetch latest emoji data and write `json_path` + sibling `emojis.version`
/// (or `version_path` when provided).
pub fn update_catalogue(json_path: &Path, version_path: Option<&Path>) -> Result<UpdateReport> {
    log::info!("emoji update: fetching {EMOJI_TEST_URL}");
    let test_text = fetch(EMOJI_TEST_URL)?;
    let (version, mut entries) = parse_emoji_test(&test_text)?;
    log::info!(
        "emoji update: Unicode {version}, {} fully-qualified",
        entries.len()
    );

    log::info!("emoji update: fetching {EMOJILIB_URL}");
    let lib_raw = fetch(EMOJILIB_URL)?;
    let lib: HashMap<String, Vec<String>> =
        serde_json::from_str(&lib_raw).context("parse emojilib JSON")?;

    let mut with_keywords = 0usize;
    for e in &mut entries {
        e.keywords = keywords_for(&e.char, &e.name, &lib);
        if !e.keywords.is_empty() {
            with_keywords += 1;
        }
    }

    if let Some(parent) = json_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_string(&entries).context("serialize emojis.json")?;
    fs::write(json_path, format!("{body}\n"))
        .with_context(|| format!("write {}", json_path.display()))?;

    let ver_path = version_path
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| {
            json_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("emojis.version")
        });
    fs::write(&ver_path, format!("{version}\n"))
        .with_context(|| format!("write {}", ver_path.display()))?;

    Ok(UpdateReport {
        version,
        count: entries.len(),
        with_keywords,
        json_path: json_path.to_path_buf(),
    })
}

/// Write into the user's XDG data dir (`~/.local/share/timbits/…`).
pub fn update_user_catalogue() -> Result<UpdateReport> {
    crate::config::ensure_dirs()?;
    let json = crate::config::emojis_json_path();
    let ver = crate::config::data_dir().join("emojis.version");
    update_catalogue(&json, Some(&ver))
}

/// Dev helper: write into the source-tree `assets/` directory.
pub fn update_workspace_assets(root: &Path) -> Result<UpdateReport> {
    let json = root.join("assets/emojis.json");
    let ver = root.join("assets/emojis.version");
    update_catalogue(&json, Some(&ver))
}

fn fetch(url: &str) -> Result<String> {
    let attempts: &[(&str, &[&str])] = &[
        ("curl", &["-fsSL", "--max-time", "90", url]),
        ("wget", &["-qO-", url]),
    ];
    let mut last_err = None;
    for (bin, args) in attempts {
        match Command::new(bin).args(*args).output() {
            Ok(out) if out.status.success() => {
                return String::from_utf8(out.stdout)
                    .with_context(|| format!("{bin} returned non-utf8 for {url}"));
            }
            Ok(out) => {
                last_err = Some(format!(
                    "{bin} failed ({}): {}",
                    out.status,
                    String::from_utf8_lossy(&out.stderr).trim()
                ));
            }
            Err(e) => {
                last_err = Some(format!("{bin}: {e}"));
            }
        }
    }
    bail!(
        "failed to fetch {url}: {} (install curl or wget)",
        last_err.unwrap_or_else(|| "no curl/wget".into())
    )
}

fn parse_emoji_test(text: &str) -> Result<(String, Vec<JsonEmoji>)> {
    let mut version = "unknown".to_string();
    for line in text.lines().take(40) {
        if let Some(rest) = line.strip_prefix("# Version:") {
            version = rest.trim().to_string();
            break;
        }
    }

    let mut group = String::new();
    let mut subgroup = String::new();
    let mut entries = Vec::new();

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("# group:") {
            group = rest.trim().to_string();
            continue;
        }
        if let Some(rest) = line.strip_prefix("# subgroup:") {
            subgroup = rest.trim().to_string();
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(caps) = parse_test_line(line) else {
            continue;
        };
        if caps.status != "fully-qualified" {
            continue;
        }
        let category = if subgroup.is_empty() {
            group.clone()
        } else {
            format!("{group} ({subgroup})")
        };
        entries.push(JsonEmoji {
            codes: caps.codes,
            char: caps.emoji,
            name: caps.name,
            category,
            group: group.clone(),
            subgroup: subgroup.clone(),
            keywords: String::new(),
        });
    }

    if entries.is_empty() {
        bail!("no fully-qualified emoji parsed from emoji-test.txt");
    }
    Ok((version, entries))
}

struct LineCaps {
    codes: String,
    status: String,
    emoji: String,
    name: String,
}

fn parse_test_line(line: &str) -> Option<LineCaps> {
    let (left, right) = line.split_once('#')?;
    let left = left.trim();
    let (codes, status) = left.split_once(';')?;
    let codes = codes.trim().to_string();
    let status = status.trim().to_string();
    let right = right.trim();
    let mut parts = right.splitn(3, char::is_whitespace);
    let emoji = parts.next()?.to_string();
    let ver = parts.next()?;
    if !ver.starts_with('E') {
        return None;
    }
    let name = parts.next()?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    Some(LineCaps {
        codes,
        status,
        emoji,
        name,
    })
}

fn strip_for_keyword_lookup(ch: &str) -> Vec<String> {
    let mut candidates = vec![ch.to_string()];
    let no_vs: String = ch.chars().filter(|&c| c != VS16).collect();
    if no_vs != ch {
        candidates.push(no_vs.clone());
    }
    let no_skin: String = no_vs
        .chars()
        .filter(|c| !SKIN_TONES.contains(c))
        .collect();
    if !no_skin.is_empty() && !candidates.iter().any(|c| c == &no_skin) {
        candidates.push(no_skin.clone());
    }
    if !no_skin.is_empty() && !no_skin.ends_with(VS16) {
        let with_vs = format!("{no_skin}{VS16}");
        if !candidates.iter().any(|c| c == &with_vs) {
            candidates.push(with_vs);
        }
    }
    candidates
}

fn keywords_for(ch: &str, name: &str, lib: &HashMap<String, Vec<String>>) -> String {
    let mut words: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let mut add_token = |tok: &str| {
        let tok = tok.trim().to_lowercase().replace('_', " ");
        if tok.is_empty() || tok == ":" || tok == "-" {
            return;
        }
        if seen.insert(tok.clone()) {
            words.push(tok);
        }
    };

    for key in strip_for_keyword_lookup(ch) {
        if let Some(kws) = lib.get(&key) {
            for kw in kws {
                add_token(kw);
                if kw.contains('_') {
                    for part in kw.split('_') {
                        add_token(part);
                    }
                }
            }
            break;
        }
    }

    for part in name
        .to_lowercase()
        .split(|c: char| c.is_whitespace() || matches!(c, ':' | ',' | '&'))
    {
        if part.len() > 1 {
            add_token(part);
        }
    }

    words.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sample_line() {
        let line = "1F1E8 1F1E6                                          ; fully-qualified     # 🇨🇦 E2.0 flag: Canada";
        let c = parse_test_line(line).expect("parse");
        assert_eq!(c.status, "fully-qualified");
        assert_eq!(c.emoji, "🇨🇦");
        assert_eq!(c.name, "flag: Canada");
    }
}
