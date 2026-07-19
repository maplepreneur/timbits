//! Load emoji catalogue from emojis.json + search aliases.

use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config;
use crate::emoji_aliases;

/// One searchable emoji.
#[derive(Debug, Clone)]
pub struct EmojiEntry {
    pub ch: String,
    pub name: String,
    pub group: String,
    /// True when the Unicode name includes a skin-tone qualifier.
    pub is_skin_tone: bool,
    /// Lowercase phone-style keywords (for ranking).
    pub keywords: String,
    /// Lowercase blob: name, group, subgroup, category, codes, keywords, aliases.
    pub search_blob: String,
}

#[derive(Debug, Deserialize)]
struct JsonEmoji {
    #[serde(default)]
    codes: String,
    char: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    group: String,
    #[serde(default)]
    subgroup: String,
    /// Phone-style keywords from emojilib / generator (optional for older JSON).
    #[serde(default)]
    keywords: String,
}

/// Bundled fallback (release builds always have a catalogue).
const BUNDLED_JSON: &str = include_str!("../assets/emojis.json");
/// Version stamp shipped with this binary (from `assets/emojis.version`).
const BUNDLED_VERSION: &str = include_str!("../assets/emojis.version");

/// Load the full catalogue. Never returns empty if the crate fallback works.
///
/// Priority:
/// 1. `TIMBITS_EMOJIS_JSON` env override
/// 2. User data dir (`~/.local/share/timbits/emojis.json`) when its version is
///    **≥** the bundled version (Settings → Update emoji catalogue, or
///    `timbits update-emojis`)
/// 3. Bundled `include_str!` catalogue
/// 4. Dev cwd / crate fallback
pub fn load() -> Vec<EmojiEntry> {
    let aliases = emoji_aliases::merged_aliases(&config::emoji_aliases_path());
    let bundled_ver = BUNDLED_VERSION.trim();

    // Explicit override for maintainers / tests.
    if let Ok(path) = std::env::var("TIMBITS_EMOJIS_JSON") {
        let path = PathBuf::from(path);
        if path.is_file() {
            match load_from_path(&path, &aliases) {
                Ok(entries) if !entries.is_empty() => {
                    log::info!(
                        "emoji db: {} entries from TIMBITS_EMOJIS_JSON={}",
                        entries.len(),
                        path.display()
                    );
                    return entries;
                }
                Ok(_) => log::warn!("TIMBITS_EMOJIS_JSON empty at {}", path.display()),
                Err(e) => log::warn!("TIMBITS_EMOJIS_JSON {}: {e}", path.display()),
            }
        }
    }

    // User-updated catalogue (Settings button / `timbits update-emojis`).
    let user_json = config::data_dir().join("emojis.json");
    let user_ver_path = config::data_dir().join("emojis.version");
    if user_json.is_file() {
        let user_ver = fs::read_to_string(&user_ver_path)
            .unwrap_or_default()
            .trim()
            .to_string();
        if !user_ver.is_empty() && version_at_least(&user_ver, bundled_ver) {
            match load_from_path(&user_json, &aliases) {
                Ok(entries) if !entries.is_empty() => {
                    log::info!(
                        "emoji db: {} entries from {} (Unicode {user_ver})",
                        entries.len(),
                        user_json.display()
                    );
                    return entries;
                }
                Ok(_) => log::warn!("emoji db empty at {}", user_json.display()),
                Err(e) => log::warn!("emoji db load {}: {e}", user_json.display()),
            }
        } else if !user_ver.is_empty() {
            log::info!(
                "emoji db: ignoring user catalogue {user_ver} (bundled {bundled_ver} is newer)"
            );
        }
    }

    match load_from_str(BUNDLED_JSON, &aliases) {
        Ok(entries) if !entries.is_empty() => {
            log::info!(
                "emoji db: {} entries from bundled assets (Unicode {bundled_ver})",
                entries.len()
            );
            return entries;
        }
        Ok(_) => log::warn!("bundled emoji db empty"),
        Err(e) => log::warn!("bundled emoji db parse: {e}"),
    }

    // Dev fallback: assets next to cwd.
    for path in [
        PathBuf::from("assets/emojis.json"),
        std::env::var("CARGO_MANIFEST_DIR")
            .map(|m| PathBuf::from(m).join("assets/emojis.json"))
            .unwrap_or_default(),
    ] {
        if !path.is_file() {
            continue;
        }
        match load_from_path(&path, &aliases) {
            Ok(entries) if !entries.is_empty() => {
                log::info!("emoji db: {} entries from {}", entries.len(), path.display());
                return entries;
            }
            Ok(_) => log::warn!("emoji db empty at {}", path.display()),
            Err(e) => log::warn!("emoji db load {}: {e}", path.display()),
        }
    }

    // Last resort: emojis crate.
    log::warn!("emoji db: falling back to emojis crate");
    load_from_crate(&aliases)
}

/// Compare dotted version strings (`"17.0"`, `"15.1"`). True when `a >= b`.
pub(crate) fn version_at_least(a: &str, b: &str) -> bool {
    fn parts(s: &str) -> Vec<u32> {
        s.trim()
            .split(|c: char| !c.is_ascii_digit())
            .filter(|p| !p.is_empty())
            .filter_map(|p| p.parse().ok())
            .collect()
    }
    let pa = parts(a);
    let pb = parts(b);
    let n = pa.len().max(pb.len());
    for i in 0..n {
        let x = pa.get(i).copied().unwrap_or(0);
        let y = pb.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    true
}

fn load_from_path(path: &Path, aliases: &HashMap<String, String>) -> anyhow::Result<Vec<EmojiEntry>> {
    let raw = fs::read_to_string(path)?;
    load_from_str(&raw, aliases)
}

fn load_from_str(raw: &str, aliases: &HashMap<String, String>) -> anyhow::Result<Vec<EmojiEntry>> {
    let items: Vec<JsonEmoji> = serde_json::from_str(raw)?;
    Ok(items
        .into_iter()
        .filter(|i| !i.char.is_empty())
        .map(|i| to_entry(i, aliases))
        .collect())
}

fn to_entry(i: JsonEmoji, aliases: &HashMap<String, String>) -> EmojiEntry {
    let is_skin_tone = i.name.to_lowercase().contains("skin tone");
    let mut parts = vec![
        i.name.clone(),
        i.group.clone(),
        i.subgroup.clone(),
        i.category.clone(),
        i.codes.clone(),
        i.keywords.clone(),
    ];
    // Auto-expand flags: "flag: Canada" → "canada flag", "canadian", etc.
    let flag_bits = flag_search_terms(&i.name, &i.group, &i.subgroup);
    if !flag_bits.is_empty() {
        parts.push(flag_bits);
    }
    // Hand / user aliases (exact char, then FE0F-stripped form).
    if let Some(extra) = alias_for(&i.char, aliases) {
        parts.push(extra);
    }
    // Also match shortcodes from emojis crate when available.
    if let Some(e) = emojis::get(&i.char) {
        for sc in e.shortcodes() {
            parts.push(sc.to_string());
        }
    } else if let Some(e) = emojis::get(strip_fe0f(&i.char).as_str()) {
        for sc in e.shortcodes() {
            parts.push(sc.to_string());
        }
    }
    let search_blob = parts.join(" ").to_lowercase();
    EmojiEntry {
        ch: i.char,
        name: i.name,
        group: i.group,
        is_skin_tone,
        keywords: i.keywords.to_lowercase(),
        search_blob,
    }
}

/// Look up aliases by exact key or with/without U+FE0F.
fn alias_for(ch: &str, aliases: &HashMap<String, String>) -> Option<String> {
    if let Some(v) = aliases.get(ch) {
        return Some(v.clone());
    }
    let stripped = strip_fe0f(ch);
    if stripped != ch {
        if let Some(v) = aliases.get(&stripped) {
            return Some(v.clone());
        }
    }
    let with = format!("{ch}\u{fe0f}");
    aliases.get(&with).cloned()
}

fn strip_fe0f(s: &str) -> String {
    s.chars().filter(|&c| c != '\u{fe0f}').collect()
}

/// Build search keywords for flag emojis.
///
/// `flag: Canada` → `canada flag`, `canadian flag`, `canada`, `canadian`, …
/// `rainbow flag` → `rainbow flag`, `pride`, …
fn flag_search_terms(name: &str, group: &str, subgroup: &str) -> String {
    let name_l = name.to_lowercase();
    let is_country_flag = name_l.starts_with("flag:");
    let is_flag_group = group.eq_ignore_ascii_case("Flags")
        || subgroup.to_lowercase().contains("flag")
        || name_l.ends_with(" flag")
        || name_l.contains(" flag ");

    if !is_country_flag && !is_flag_group {
        return String::new();
    }

    let mut terms: Vec<String> = vec!["flag".into()];

    if let Some(rest) = name_l.strip_prefix("flag:") {
        let region = rest.trim();
        // "Canada flag", "flag of Canada", bare "Canada"
        terms.push(format!("{region} flag"));
        terms.push(format!("flag of {region}"));
        terms.push(region.to_string());

        // Split multi-word regions so "united states" also matches "states" weakly
        // via full phrase tokens (we still require AND across user words).
        for piece in region.split(|c: char| c == ' ' || c == ',' || c == '-') {
            let piece = piece.trim();
            if piece.len() > 2 && piece != "and" && piece != "the" {
                terms.push(piece.to_string());
            }
        }

        // Ampersand variants: "Antigua & Barbuda" → "antigua and barbuda"
        if region.contains('&') {
            let anded = region.replace('&', "and");
            terms.push(anded.clone());
            terms.push(format!("{anded} flag"));
        }

        terms.push(demonyms_for_region(region));
    } else {
        // Named flags: "rainbow flag", "chequered flag", "transgender flag"
        terms.push(name_l.clone());
        if let Some(base) = name_l.strip_suffix(" flag") {
            terms.push(base.to_string());
            terms.push(format!("{base} flag"));
        }
        match name_l.as_str() {
            "rainbow flag" => terms.push("pride lgbt lgbtq gay".into()),
            "transgender flag" => terms.push("trans pride lgbt".into()),
            "pirate flag" => terms.push("pirate jolly roger".into()),
            "chequered flag" | "checkered flag" => {
                terms.push("racing finish race checkered chequered".into())
            }
            "triangular flag" => terms.push("red flag warning".into()),
            "white flag" => terms.push("surrender peace truce".into()),
            "black flag" => terms.push("pirate anarchist".into()),
            _ => {}
        }
    }

    terms.join(" ")
}

/// Demonyms and common alternate names for regions in `flag: …` titles.
fn demonyms_for_region(region: &str) -> String {
    // region is already lowercased
    let extra = match region {
        "canada" => "canadian canada canadian flag",
        "united states" => {
            "usa us america american americans united states of america usa flag american flag"
        }
        "united kingdom" => {
            "uk britain british great britain england gbr united kingdom flag british flag"
        }
        "england" => "english england english flag",
        "scotland" => "scottish scotland scottish flag",
        "wales" => "welsh wales welsh flag",
        "australia" => "australian australia aussie australian flag",
        "new zealand" => "kiwi new zealand nz new zealander",
        "ireland" => "irish ireland irish flag",
        "france" => "french france french flag",
        "germany" => "german germany deutschland german flag",
        "spain" => "spanish spain espanol spanish flag",
        "italy" => "italian italy italian flag",
        "portugal" => "portuguese portugal portuguese flag",
        "netherlands" => "dutch holland netherlands dutch flag",
        "belgium" => "belgian belgium belgian flag",
        "switzerland" => "swiss switzerland swiss flag",
        "austria" => "austrian austria austrian flag",
        "sweden" => "swedish sweden swedish flag",
        "norway" => "norwegian norway norwegian flag",
        "denmark" => "danish denmark danish flag",
        "finland" => "finnish finland finnish flag",
        "iceland" => "icelandic iceland icelandic flag",
        "poland" => "polish poland polish flag",
        "ukraine" => "ukrainian ukraine ukrainian flag",
        "russia" => "russian russia russian flag",
        "china" => "chinese china prc chinese flag",
        "japan" => "japanese japan japanese flag",
        "south korea" => "korean korea south korea korean flag",
        "north korea" => "north korean dprk",
        "india" => "indian india indian flag",
        "pakistan" => "pakistani pakistan pakistani flag",
        "bangladesh" => "bangladeshi bangladesh",
        "indonesia" => "indonesian indonesia",
        "philippines" => "filipino philippines pinoy",
        "vietnam" => "vietnamese vietnam",
        "thailand" => "thai thailand",
        "malaysia" => "malaysian malaysia",
        "singapore" => "singaporean singapore",
        "mexico" => "mexican mexico mexican flag",
        "brazil" => "brazilian brazil brazilian flag",
        "argentina" => "argentinian argentine argentina",
        "chile" => "chilean chile",
        "colombia" => "colombian colombia",
        "peru" => "peruvian peru",
        "venezuela" => "venezuelan venezuela",
        "cuba" => "cuban cuba",
        "jamaica" => "jamaican jamaica",
        "south africa" => "south african south africa",
        "egypt" => "egyptian egypt",
        "nigeria" => "nigerian nigeria",
        "kenya" => "kenyan kenya",
        "ethiopia" => "ethiopian ethiopia",
        "ghana" => "ghanaian ghana",
        "morocco" => "moroccan morocco",
        "turkey" => "turkish turkey turkiye",
        "saudi arabia" => "saudi saudi arabian",
        "united arab emirates" => "uae emirates emirati",
        "israel" => "israeli israel",
        "palestine" => "palestinian palestine",
        "iran" => "iranian iran persia persian",
        "iraq" => "iraqi iraq",
        "greece" => "greek greece hellenic",
        "czech republic" | "czechia" => "czech czechia czech republic",
        "slovakia" => "slovak slovakia",
        "hungary" => "hungarian hungary",
        "romania" => "romanian romania",
        "bulgaria" => "bulgarian bulgaria",
        "serbia" => "serbian serbia",
        "croatia" => "croatian croatia",
        "slovenia" => "slovenian slovenia",
        "bosnia & herzegovina" | "bosnia and herzegovina" => "bosnian bosnia",
        "albania" => "albanian albania",
        "north macedonia" | "macedonia" => "macedonian macedonia",
        "georgia" => "georgian georgia",
        "armenia" => "armenian armenia",
        "azerbaijan" => "azerbaijani azerbaijan",
        "kazakhstan" => "kazakh kazakhstan",
        "uzbekistan" => "uzbek uzbekistan",
        "taiwan" => "taiwanese taiwan roc",
        "hong kong" => "hongkonger hong kong hk",
        "macao" | "macau" => "macanese macau macao",
        "puerto rico" => "puerto rican puerto rico",
        "greenland" => "greenlandic greenland",
        "antarctica" => "antarctic antarctica",
        "european union" => "eu europe european union european",
        "united nations" => "un united nations",
        _ => "",
    };
    extra.to_string()
}

fn load_from_crate(aliases: &HashMap<String, String>) -> Vec<EmojiEntry> {
    emojis::iter()
        .map(|e| {
            let ch = e.as_str().to_string();
            let name = e.name().to_string();
            let is_skin_tone = name.to_lowercase().contains("skin tone");
            let mut parts = vec![name.clone()];
            for sc in e.shortcodes() {
                parts.push(sc.to_string());
            }
            if let Some(extra) = alias_for(&ch, aliases) {
                parts.push(extra);
            }
            EmojiEntry {
                ch,
                name,
                group: String::new(),
                is_skin_tone,
                keywords: String::new(),
                search_blob: parts.join(" ").to_lowercase(),
            }
        })
        .collect()
}

/// Score a match for ranking (higher is better).
fn match_score(e: &EmojiEntry, words: &[&str], query: &str) -> i32 {
    let name = e.name.to_lowercase();
    let mut score = 0i32;

    // Prefer non–skin-tone glyphs unless the user asked for a tone.
    let wants_tone = words.iter().any(|w| {
        matches!(
            *w,
            "skin" | "tone" | "light" | "medium" | "dark" | "medium-light" | "medium-dark"
        ) || w.contains("skin")
    });
    if e.is_skin_tone && !wants_tone {
        score -= 50;
    } else if !e.is_skin_tone {
        score += 10;
    }

    if query.is_empty() {
        return score;
    }

    if name == query {
        score += 200;
    } else if name.starts_with(query) {
        score += 120;
    } else if name.contains(query) {
        score += 80;
    }

    // Whole-word hits in the official name beat keyword-only hits.
    let name_tokens: Vec<&str> = name
        .split(|c: char| !c.is_alphanumeric() && c != '+')
        .filter(|t| !t.is_empty())
        .collect();
    let name_word_hits = words
        .iter()
        .filter(|w| name_tokens.iter().any(|t| t == *w || t.starts_with(*w)))
        .count();
    score += (name_word_hits as i32) * 45;

    // Whole-word hits in keywords (phone-style aliases). Prefer early keywords.
    let kw = e.keywords.as_str();
    if !kw.is_empty() {
        let kw_tokens: Vec<&str> = kw.split_whitespace().collect();
        for (i, w) in words.iter().enumerate() {
            if let Some(pos) = kw_tokens.iter().position(|t| t == w || t.replace('_', "") == *w)
            {
                // Earlier keyword tokens score higher (emojilib puts best aliases first).
                score += 55 - (pos.min(20) as i32);
                if i == 0 && pos < 5 {
                    score += 25;
                }
            } else if kw.contains(w) {
                score += 8;
            }
        }
    }

    // Prefer shorter official names (less compound / modifier noise).
    score += (40 - name.len().min(40) as i32).max(0) / 4;

    // Flags often match demonyms via keywords; slight boost so 🇨🇦 beats 🍁 for "canada".
    if e.group.eq_ignore_ascii_case("Flags") && name_word_hits + words.len() > 0 {
        let region = name.strip_prefix("flag: ").unwrap_or("");
        if words.iter().any(|w| region.contains(w) || kw.split_whitespace().any(|t| t == *w)) {
            score += 35;
        }
    }

    score
}

/// Filter entries: every whitespace token must appear in `search_blob`.
/// Results are ranked (name hits first). Skin-tone *variants* are hidden
/// unless the query explicitly asks for a tone (or `include_skin_tones`).
pub fn filter<'a>(all: &'a [EmojiEntry], query: &str, limit: usize) -> Vec<&'a EmojiEntry> {
    filter_opts(all, query, limit, false)
}

/// Like [`filter`], with control over whether named skin-tone rows appear.
pub fn filter_opts<'a>(
    all: &'a [EmojiEntry],
    query: &str,
    limit: usize,
    include_skin_tones: bool,
) -> Vec<&'a EmojiEntry> {
    let q = query.trim().to_lowercase();
    let words: Vec<&str> = q.split_whitespace().filter(|w| !w.is_empty()).collect();
    let wants_tone = words.iter().any(|w| {
        matches!(
            *w,
            "skin" | "tone" | "light" | "medium" | "dark" | "medium-light" | "medium-dark"
        ) || w.contains("skin")
    });
    let show_tones = include_skin_tones || wants_tone;

    let mut scored: Vec<(i32, usize, &EmojiEntry)> = Vec::new();
    for (idx, e) in all.iter().enumerate() {
        if e.is_skin_tone && !show_tones {
            continue;
        }
        if words.is_empty() || words.iter().all(|w| e.search_blob.contains(w)) {
            let score = match_score(e, &words, &q);
            scored.push((score, idx, e));
        }
    }
    // Higher score first; stable catalogue order as tiebreaker (lower idx).
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    scored.into_iter().take(limit).map(|(_, _, e)| e).collect()
}

/// Resolve `base` to the preferred skin-tone form when one exists in the catalogue.
///
/// Looks up `"thumbs up: medium skin tone"` from base name `"thumbs up"`. If the
/// base has no such variant (or tone is [`SkinTone::None`]), returns `base.ch`.
pub fn with_skin_tone(base: &EmojiEntry, all: &[EmojiEntry], tone: crate::config::SkinTone) -> String {
    let Some(suffix) = tone.name_suffix() else {
        return base.ch.clone();
    };
    // Exact same-tone name: "waving hand: medium skin tone"
    let want = format!("{}: {}", base.name, suffix);
    if let Some(e) = all.iter().find(|e| e.name.eq_ignore_ascii_case(&want)) {
        return e.ch.clone();
    }
    // Multi-person mixed names only (skip) — prefer leaving the base form.
    base.ch.clone()
}

/// Browse helpers: default-skin (no tone in name) first, catalogue order.
pub fn browse_default_skin<'a>(all: &'a [EmojiEntry], limit: usize) -> Vec<&'a EmojiEntry> {
    let mut out = Vec::new();
    for e in all {
        if e.is_skin_tone {
            continue;
        }
        out.push(e);
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// Filter by Unicode group name (exact match, case-insensitive).
pub fn by_group<'a>(
    all: &'a [EmojiEntry],
    group: &str,
    default_skin_only: bool,
    limit: usize,
) -> Vec<&'a EmojiEntry> {
    let mut out = Vec::new();
    for e in all {
        if !e.group.eq_ignore_ascii_case(group) {
            continue;
        }
        if default_skin_only && e.is_skin_tone {
            continue;
        }
        out.push(e);
        if out.len() >= limit {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded() -> Vec<EmojiEntry> {
        let aliases = emoji_aliases::builtin_aliases();
        load_from_str(BUNDLED_JSON, &aliases).expect("parse bundled")
    }

    #[test]
    fn bundled_is_full_unicode_set() {
        let all = loaded();
        assert!(
            all.len() >= 3900,
            "expected full FQ catalogue, got {}",
            all.len()
        );
    }

    #[test]
    fn bundled_loads_and_finds_grinning() {
        let all = loaded();
        let hits = filter(&all, "grinning face", 20);
        assert!(!hits.is_empty());
        assert!(hits
            .iter()
            .any(|e| e.ch.contains('😀') || e.name.contains("grinning")));
    }

    #[test]
    fn donut_alias_matches() {
        let all = loaded();
        let hits = filter(&all, "donut", 10);
        assert!(
            hits.iter().any(|e| e.ch == "🍩"),
            "expected 🍩 in {:?}",
            hits.iter().map(|e| &e.ch).collect::<Vec<_>>()
        );
    }

    #[test]
    fn multi_word_and() {
        let all = loaded();
        let hits = filter(&all, "face smiling", 50);
        assert!(!hits.is_empty());
    }

    #[test]
    fn canada_matches_canadian_flag() {
        let all = loaded();
        for q in ["canada", "canadian", "canadian flag", "canada flag"] {
            let hits = filter(&all, q, 20);
            assert!(
                hits.iter().any(|e| e.ch == "🇨🇦"),
                "query {q:?} expected 🇨🇦 in {:?}",
                hits.iter().map(|e| (&e.ch, &e.name)).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn usa_and_american_match_us_flag() {
        let all = loaded();
        for q in ["usa", "america", "american", "united states"] {
            let hits = filter(&all, q, 20);
            assert!(
                hits.iter().any(|e| e.ch == "🇺🇸"),
                "query {q:?} expected 🇺🇸 in {:?}",
                hits.iter().map(|e| &e.ch).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn flag_terms_from_name() {
        let t = flag_search_terms("flag: Canada", "Flags", "country-flag");
        let t = t.to_lowercase();
        assert!(t.contains("canada"));
        assert!(t.contains("canadian"));
        assert!(t.contains("flag"));
    }

    #[test]
    fn phone_style_keywords() {
        let all = loaded();
        // emojilib / aliases
        for (q, ch) in [
            ("happy", "😀"),
            ("coffee", "☕"),
            ("lol", "😂"),
            ("poop", "💩"),
        ] {
            let hits = filter(&all, q, 30);
            assert!(
                hits.iter().any(|e| e.ch == ch || e.ch.contains(ch)),
                "query {q:?} expected something like {ch} in {:?}",
                hits.iter().map(|e| (&e.ch, &e.name)).collect::<Vec<_>>()
            );
        }
        // pride → rainbow flag should rank highly
        let hits = filter(&all, "pride", 15);
        assert!(
            hits.iter()
                .any(|e| e.name.to_lowercase().contains("rainbow flag")),
            "pride should find rainbow flag in {:?}",
            hits.iter().map(|e| &e.name).collect::<Vec<_>>()
        );
        assert!(
            hits[0].name.to_lowercase().contains("rainbow")
                || hits[0].name.to_lowercase().contains("transgender"),
            "pride should rank a pride flag first, got {}",
            hits[0].name
        );
    }

    #[test]
    fn canada_ranks_flag_above_maple() {
        let all = loaded();
        let hits = filter(&all, "canada", 10);
        assert_eq!(hits[0].ch, "🇨🇦", "top hit should be 🇨🇦, got {}", hits[0].name);
    }

    #[test]
    fn unicode17_distorted_face() {
        let all = loaded();
        assert!(
            all.iter().any(|e| e.name == "distorted face"),
            "U17 distorted face missing"
        );
        let hits = filter(&all, "distorted", 10);
        assert!(hits.iter().any(|e| e.name == "distorted face"));
    }

    #[test]
    fn search_hides_skin_tone_variants() {
        let all = loaded();
        let hits = filter(&all, "thumbs up", 50);
        assert!(!hits.is_empty());
        assert!(
            hits.iter().all(|e| !e.is_skin_tone),
            "skin-tone variants must not appear in normal search: {:?}",
            hits.iter().map(|e| &e.name).collect::<Vec<_>>()
        );
        assert!(hits.iter().any(|e| e.ch == "👍" || e.name == "thumbs up"));
    }

    #[test]
    fn with_skin_tone_applies_medium() {
        use crate::config::SkinTone;
        let all = loaded();
        let base = all
            .iter()
            .find(|e| e.name == "thumbs up")
            .expect("thumbs up");
        assert_eq!(with_skin_tone(base, &all, SkinTone::None), "👍");
        assert_eq!(with_skin_tone(base, &all, SkinTone::Medium), "👍🏽");
        assert_eq!(with_skin_tone(base, &all, SkinTone::Light), "👍🏻");
    }

    #[test]
    fn skin_tone_query_can_still_find_variants() {
        let all = loaded();
        let hits = filter(&all, "thumbs up light skin", 20);
        assert!(
            hits.iter().any(|e| e.is_skin_tone),
            "explicit tone query should surface variants"
        );
    }

    #[test]
    fn group_filter_flags() {
        let all = loaded();
        let flags = by_group(&all, "Flags", true, 500);
        assert!(flags.len() > 200);
        assert!(flags.iter().any(|e| e.ch == "🇨🇦"));
    }

    #[test]
    fn version_compare() {
        assert!(version_at_least("17.0", "17.0"));
        assert!(version_at_least("17.0", "16.0"));
        assert!(version_at_least("17.1", "17.0"));
        assert!(!version_at_least("16.0", "17.0"));
        assert!(version_at_least("17.0\n", "17.0"));
    }
}
