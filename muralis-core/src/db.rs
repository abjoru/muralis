use rusqlite::{params, Connection};

use crate::error::{MuralisError, Result};
use crate::models::{BlacklistEntry, SourceType, Wallpaper};

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS wallpapers (
                id TEXT PRIMARY KEY,
                source_type TEXT NOT NULL,
                source_id TEXT NOT NULL,
                source_url TEXT,
                width INTEGER NOT NULL,
                height INTEGER NOT NULL,
                tags TEXT NOT NULL DEFAULT '[]',
                file_path TEXT NOT NULL,
                added_at TEXT NOT NULL,
                last_used TEXT,
                use_count INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS blacklist (
                source_id TEXT NOT NULL,
                source TEXT NOT NULL,
                blacklisted_at TEXT NOT NULL,
                PRIMARY KEY (source_id, source)
            );",
        )?;
        Ok(())
    }

    // -- Wallpaper CRUD --

    pub fn insert_wallpaper(&self, wp: &Wallpaper) -> Result<()> {
        let tags_json = serde_json::to_string(&wp.tags)?;
        self.conn.execute(
            "INSERT OR REPLACE INTO wallpapers
             (id, source_type, source_id, source_url, width, height, tags, file_path, added_at, last_used, use_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                wp.id,
                wp.source_type.to_string(),
                wp.source_id,
                wp.source_url,
                wp.width,
                wp.height,
                tags_json,
                wp.file_path,
                wp.added_at,
                wp.last_used,
                wp.use_count,
            ],
        )?;
        Ok(())
    }

    pub fn get_wallpaper(&self, id: &str) -> Result<Wallpaper> {
        let wp = self.conn.query_row(
            "SELECT id, source_type, source_id, source_url, width, height, tags, file_path, added_at, last_used, use_count
             FROM wallpapers WHERE id = ?1",
            params![id],
            |row| {
                let tags_str: String = row.get(6)?;
                let source_str: String = row.get(1)?;
                Ok(WallpaperRow {
                    id: row.get(0)?,
                    source_type: source_str,
                    source_id: row.get(2)?,
                    source_url: row.get(3)?,
                    width: row.get(4)?,
                    height: row.get(5)?,
                    tags: tags_str,
                    file_path: row.get(7)?,
                    added_at: row.get(8)?,
                    last_used: row.get(9)?,
                    use_count: row.get(10)?,
                })
            },
        ).map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => MuralisError::WallpaperNotFound(id.to_string()),
            other => MuralisError::Database(other),
        })?;
        row_to_wallpaper(wp)
    }

    pub fn list_wallpapers(&self) -> Result<Vec<Wallpaper>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source_type, source_id, source_url, width, height, tags, file_path, added_at, last_used, use_count
             FROM wallpapers ORDER BY added_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let tags_str: String = row.get(6)?;
            let source_str: String = row.get(1)?;
            Ok(WallpaperRow {
                id: row.get(0)?,
                source_type: source_str,
                source_id: row.get(2)?,
                source_url: row.get(3)?,
                width: row.get(4)?,
                height: row.get(5)?,
                tags: tags_str,
                file_path: row.get(7)?,
                added_at: row.get(8)?,
                last_used: row.get(9)?,
                use_count: row.get(10)?,
            })
        })?;
        let mut wallpapers = Vec::new();
        for row in rows {
            wallpapers.push(row_to_wallpaper(row?)?);
        }
        Ok(wallpapers)
    }

    pub fn delete_wallpaper(&self, id: &str) -> Result<bool> {
        let count = self.conn.execute(
            "DELETE FROM wallpapers WHERE id = ?1",
            params![id],
        )?;
        Ok(count > 0)
    }

    pub fn mark_used(&self, id: &str) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "UPDATE wallpapers SET last_used = ?1, use_count = use_count + 1 WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    }

    pub fn wallpaper_count(&self) -> Result<u32> {
        let count: u32 = self.conn.query_row(
            "SELECT COUNT(*) FROM wallpapers",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    pub fn wallpaper_exists(&self, id: &str) -> Result<bool> {
        let count: u32 = self.conn.query_row(
            "SELECT COUNT(*) FROM wallpapers WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    // -- Blacklist CRUD --

    pub fn add_blacklist(&self, source_id: &str, source: &SourceType) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT OR IGNORE INTO blacklist (source_id, source, blacklisted_at) VALUES (?1, ?2, ?3)",
            params![source_id, source.to_string(), now],
        )?;
        Ok(())
    }

    pub fn remove_blacklist(&self, source_id: &str, source: &SourceType) -> Result<bool> {
        let count = self.conn.execute(
            "DELETE FROM blacklist WHERE source_id = ?1 AND source = ?2",
            params![source_id, source.to_string()],
        )?;
        Ok(count > 0)
    }

    pub fn is_blacklisted(&self, source_id: &str, source: &SourceType) -> Result<bool> {
        let count: u32 = self.conn.query_row(
            "SELECT COUNT(*) FROM blacklist WHERE source_id = ?1 AND source = ?2",
            params![source_id, source.to_string()],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn list_blacklist(&self) -> Result<Vec<BlacklistEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT source_id, source, blacklisted_at FROM blacklist ORDER BY blacklisted_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let source_str: String = row.get(1)?;
            Ok((row.get::<_, String>(0)?, source_str, row.get::<_, String>(2)?))
        })?;
        let mut entries = Vec::new();
        for row in rows {
            let (source_id, source_str, blacklisted_at) = row?;
            let source: SourceType = source_str
                .parse()
                .map_err(|e: String| rusqlite::Error::InvalidParameterName(e))?;
            entries.push(BlacklistEntry {
                source_id,
                source,
                blacklisted_at,
            });
        }
        Ok(entries)
    }
}

// Internal helper types

struct WallpaperRow {
    id: String,
    source_type: String,
    source_id: String,
    source_url: Option<String>,
    width: u32,
    height: u32,
    tags: String,
    file_path: String,
    added_at: String,
    last_used: Option<String>,
    use_count: u32,
}

fn row_to_wallpaper(row: WallpaperRow) -> Result<Wallpaper> {
    let source_type: SourceType = row
        .source_type
        .parse()
        .map_err(|e: String| MuralisError::Database(rusqlite::Error::InvalidParameterName(e)))?;
    let tags: Vec<String> = serde_json::from_str(&row.tags)?;
    Ok(Wallpaper {
        id: row.id,
        source_type,
        source_id: row.source_id,
        source_url: row.source_url,
        width: row.width,
        height: row.height,
        tags,
        file_path: row.file_path,
        added_at: row.added_at,
        last_used: row.last_used,
        use_count: row.use_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SourceType;

    fn test_wallpaper(id: &str) -> Wallpaper {
        Wallpaper {
            id: id.to_string(),
            source_type: SourceType::Wallhaven,
            source_id: "wh_123".into(),
            source_url: Some("https://wallhaven.cc/w/123".into()),
            width: 1920,
            height: 1080,
            tags: vec!["nature".into(), "landscape".into()],
            file_path: "/data/wallpapers/abc123.jpg".into(),
            added_at: "2025-01-01T00:00:00Z".into(),
            last_used: None,
            use_count: 0,
        }
    }

    #[test]
    fn test_insert_and_get() {
        let db = Database::open_in_memory().unwrap();
        let wp = test_wallpaper("abc123");
        db.insert_wallpaper(&wp).unwrap();

        let loaded = db.get_wallpaper("abc123").unwrap();
        assert_eq!(loaded.id, "abc123");
        assert_eq!(loaded.source_type, SourceType::Wallhaven);
        assert_eq!(loaded.tags, vec!["nature", "landscape"]);
        assert_eq!(loaded.width, 1920);
    }

    #[test]
    fn test_list_wallpapers() {
        let db = Database::open_in_memory().unwrap();
        db.insert_wallpaper(&test_wallpaper("a")).unwrap();
        db.insert_wallpaper(&test_wallpaper("b")).unwrap();

        let list = db.list_wallpapers().unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_delete_wallpaper() {
        let db = Database::open_in_memory().unwrap();
        db.insert_wallpaper(&test_wallpaper("del")).unwrap();
        assert!(db.wallpaper_exists("del").unwrap());

        assert!(db.delete_wallpaper("del").unwrap());
        assert!(!db.wallpaper_exists("del").unwrap());
        assert!(!db.delete_wallpaper("del").unwrap());
    }

    #[test]
    fn test_mark_used() {
        let db = Database::open_in_memory().unwrap();
        db.insert_wallpaper(&test_wallpaper("used")).unwrap();

        db.mark_used("used").unwrap();
        let wp = db.get_wallpaper("used").unwrap();
        assert_eq!(wp.use_count, 1);
        assert!(wp.last_used.is_some());

        db.mark_used("used").unwrap();
        let wp = db.get_wallpaper("used").unwrap();
        assert_eq!(wp.use_count, 2);
    }

    #[test]
    fn test_wallpaper_not_found() {
        let db = Database::open_in_memory().unwrap();
        let err = db.get_wallpaper("nonexistent").unwrap_err();
        assert!(matches!(err, MuralisError::WallpaperNotFound(_)));
    }

    #[test]
    fn test_blacklist_crud() {
        let db = Database::open_in_memory().unwrap();
        let source = SourceType::Wallhaven;

        assert!(!db.is_blacklisted("wh_bad", &source).unwrap());

        db.add_blacklist("wh_bad", &source).unwrap();
        assert!(db.is_blacklisted("wh_bad", &source).unwrap());

        let list = db.list_blacklist().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].source_id, "wh_bad");

        assert!(db.remove_blacklist("wh_bad", &source).unwrap());
        assert!(!db.is_blacklisted("wh_bad", &source).unwrap());
    }

    #[test]
    fn test_wallpaper_count() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(db.wallpaper_count().unwrap(), 0);

        db.insert_wallpaper(&test_wallpaper("c1")).unwrap();
        db.insert_wallpaper(&test_wallpaper("c2")).unwrap();
        assert_eq!(db.wallpaper_count().unwrap(), 2);
    }

    #[test]
    fn test_upsert_wallpaper() {
        let db = Database::open_in_memory().unwrap();
        let mut wp = test_wallpaper("upsert");
        db.insert_wallpaper(&wp).unwrap();

        wp.tags = vec!["updated".into()];
        db.insert_wallpaper(&wp).unwrap();

        let loaded = db.get_wallpaper("upsert").unwrap();
        assert_eq!(loaded.tags, vec!["updated"]);
        assert_eq!(db.wallpaper_count().unwrap(), 1);
    }
}
