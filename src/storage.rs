//! Clipboard history storage backed by SQLite.

use anyhow::Result;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Text,
    Image,
    Files,
}

impl EntryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntryKind::Text => "text",
            EntryKind::Image => "image",
            EntryKind::Files => "files",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "image" => EntryKind::Image,
            "files" => EntryKind::Files,
            _ => EntryKind::Text,
        }
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // some fields are kept for future UI / debugging
pub struct Entry {
    pub id: i64,
    pub kind: EntryKind,
    /// Text content (for text/files entries).
    pub text: Option<String>,
    /// OCR'd text (for image entries), makes screenshots searchable.
    pub ocr_text: Option<String>,
    /// PNG path (for image entries).
    pub image_path: Option<String>,
    /// Short single-line preview for list display.
    pub preview: String,
    pub hash: String,
    pub created_at: i64,
    pub last_used_at: i64,
    pub use_count: i64,
}

const COLS: &str =
    "id, kind, text, ocr_text, image_path, preview, hash, created_at, last_used_at, use_count";

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS entries (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    kind         TEXT NOT NULL,
    text         TEXT,
    ocr_text     TEXT,
    image_path   TEXT,
    preview      TEXT NOT NULL DEFAULT '',
    hash         TEXT NOT NULL UNIQUE,
    created_at   INTEGER NOT NULL,
    last_used_at INTEGER NOT NULL,
    use_count    INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX IF NOT EXISTS idx_entries_last_used ON entries(last_used_at DESC);
";

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Insert a new entry, or (if the content hash already exists) bump its
    /// last_used_at / use_count. Returns the entry id.
    pub fn upsert(
        &self,
        kind: EntryKind,
        text: Option<&str>,
        image_path: Option<&str>,
        hash: &str,
        preview: &str,
    ) -> Result<i64> {
        let now = now_ts();
        self.conn.execute(
            "INSERT INTO entries (kind, text, image_path, preview, hash, created_at, last_used_at, use_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, 1)
             ON CONFLICT(hash) DO UPDATE SET
                last_used_at = excluded.last_used_at,
                use_count = use_count + 1",
            params![kind.as_str(), text, image_path, preview, hash, now],
        )?;
        let id = self.conn.query_row(
            "SELECT id FROM entries WHERE hash = ?1",
            params![hash],
            |r| r.get(0),
        )?;
        Ok(id)
    }

    /// Search entries newest-first. All whitespace-separated words must match
    /// (against text, OCR text and preview). Empty query returns everything.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Entry>> {
        let words: Vec<String> = query
            .split_whitespace()
            .map(|w| format!("%{}%", w.to_lowercase().replace(['%', '_'], " ")))
            .collect();

        let mut sql = format!("SELECT {COLS} FROM entries");
        let mut values: Vec<String> = Vec::new();
        for w in &words {
            sql.push_str(
                if values.is_empty() { " WHERE (" } else { " AND (" },
            );
            sql.push_str(
                "lower(coalesce(text, '')) LIKE ? \
                 OR lower(coalesce(ocr_text, '')) LIKE ? \
                 OR lower(preview) LIKE ?)",
            );
            values.push(w.clone());
            values.push(w.clone());
            values.push(w.clone());
        }
        sql.push_str(" ORDER BY last_used_at DESC LIMIT ?");
        values.push((limit as i64).to_string());

        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(values.iter()), map_entry)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn set_ocr(&self, id: i64, ocr_text: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE entries SET ocr_text = ?1 WHERE id = ?2",
            params![ocr_text, id],
        )?;
        Ok(())
    }

    /// Bump last_used_at / use_count when the user pastes an entry.
    pub fn touch(&self, id: i64) -> Result<()> {
        let now = now_ts();
        self.conn.execute(
            "UPDATE entries SET last_used_at = ?1, use_count = use_count + 1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    /// Delete an entry; returns its image path (so the caller can unlink it).
    pub fn delete(&self, id: i64) -> Result<Option<String>> {
        let path: Option<Option<String>> = self
            .conn
            .query_row(
                "DELETE FROM entries WHERE id = ?1 RETURNING image_path",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(path.flatten())
    }

    /// Keep only the newest `max` entries; returns image paths to unlink.
    pub fn trim(&self, max: i64) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT image_path FROM entries
             WHERE image_path IS NOT NULL
               AND id NOT IN (SELECT id FROM entries ORDER BY last_used_at DESC LIMIT ?1)",
        )?;
        let paths: Vec<String> = stmt
            .query_map(params![max], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        self.conn.execute(
            "DELETE FROM entries
             WHERE id NOT IN (SELECT id FROM entries ORDER BY last_used_at DESC LIMIT ?1)",
            params![max],
        )?;
        Ok(paths)
    }

    #[allow(dead_code)] // used in tests
    pub fn count(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT count(*) FROM entries", [], |r| r.get(0))?)
    }
}

fn map_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<Entry> {
    Ok(Entry {
        id: row.get(0)?,
        kind: EntryKind::from_str(&row.get::<_, String>(1)?),
        text: row.get(2)?,
        ocr_text: row.get(3)?,
        image_path: row.get(4)?,
        preview: row.get(5)?,
        hash: row.get(6)?,
        created_at: row.get(7)?,
        last_used_at: row.get(8)?,
        use_count: row.get(9)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "timbits-test-{}-{}.db",
            name,
            std::process::id()
        ));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn upsert_dedupes_and_searches() -> Result<()> {
        let path = temp_db("upsert");
        let store = Store::open(&path)?;

        let id1 = store.upsert(EntryKind::Text, Some("hello world"), None, "h1", "hello world")?;
        store.upsert(EntryKind::Text, Some("rust crab"), None, "h2", "rust crab")?;
        // Duplicate hash: should not create a new row.
        let id1b = store.upsert(EntryKind::Text, Some("hello world"), None, "h1", "hello world")?;
        assert_eq!(id1, id1b);
        assert_eq!(store.count()?, 2);

        let hits = store.search("hello", 10)?;
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].text.as_deref(), Some("hello world"));

        // Newest first: the deduped "hello" was just bumped.
        let all = store.search("", 10)?;
        assert_eq!(all[0].hash, "h1");

        // Multi-word AND search.
        assert_eq!(store.search("hello rust", 10)?.len(), 0);
        assert_eq!(store.search("hello world", 10)?.len(), 1);

        std::fs::remove_file(&path).ok();
        Ok(())
    }

    #[test]
    fn ocr_text_is_searchable() -> Result<()> {
        let path = temp_db("ocr");
        let store = Store::open(&path)?;
        let id = store.upsert(EntryKind::Image, None, Some("/tmp/x.png"), "h9", "Image 10x10")?;
        store.set_ocr(id, "Invoice #42 from Acme")?;
        assert_eq!(store.search("invoice", 10)?.len(), 1);
        assert_eq!(store.search("acme", 10)?.len(), 1);
        std::fs::remove_file(&path).ok();
        Ok(())
    }

    #[test]
    fn trim_keeps_newest() -> Result<()> {
        let path = temp_db("trim");
        let store = Store::open(&path)?;
        for i in 0..10 {
            store.upsert(
                EntryKind::Text,
                Some(&format!("entry {i}")),
                None,
                &format!("h{i}"),
                &format!("entry {i}"),
            )?;
        }
        store.trim(3)?;
        assert_eq!(store.count()?, 3);
        std::fs::remove_file(&path).ok();
        Ok(())
    }

    #[test]
    fn touch_bumps_recency() -> Result<()> {
        let path = temp_db("touch");
        let store = Store::open(&path)?;
        let old = store.upsert(EntryKind::Text, Some("older"), None, "ha", "older")?;
        std::thread::sleep(std::time::Duration::from_millis(20));
        let _new = store.upsert(EntryKind::Text, Some("newer"), None, "hb", "newer")?;
        // Touch the older entry — it should sort first.
        store.touch(old)?;
        let all = store.search("", 10)?;
        assert_eq!(all[0].id, old);
        assert!(all[0].use_count >= 2);
        std::fs::remove_file(&path).ok();
        Ok(())
    }
}
