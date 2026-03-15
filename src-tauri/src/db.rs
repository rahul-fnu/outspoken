use rusqlite::Connection;
use std::path::PathBuf;

fn data_dir() -> Result<PathBuf, String> {
    let data_dir = dirs::data_dir().ok_or("Could not determine platform data directory")?;
    let dir = data_dir.join("outspoken");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create data directory: {e}"))?;
    Ok(dir)
}

fn db_path() -> Result<PathBuf, String> {
    Ok(data_dir()?.join("outspoken.db"))
}

/// Open the unified database and ensure all tables exist.
pub fn open_db() -> Result<Connection, String> {
    let path = db_path()?;
    let conn =
        Connection::open(&path).map_err(|e| format!("Failed to open database: {e}"))?;

    // Enable WAL mode for better concurrent access
    conn.execute_batch("PRAGMA journal_mode=WAL;")
        .map_err(|e| format!("Failed to set journal mode: {e}"))?;

    // Create all tables
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS models (
            name TEXT PRIMARY KEY,
            filename TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            path TEXT NOT NULL,
            version TEXT NOT NULL,
            downloaded_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS dictionary (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            from_text TEXT NOT NULL,
            to_text TEXT NOT NULL,
            case_sensitive INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS transcriptions (
            id TEXT PRIMARY KEY,
            text TEXT NOT NULL,
            raw_text TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            duration_ms INTEGER,
            source_app TEXT,
            language TEXT,
            model_used TEXT,
            is_bookmarked INTEGER DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_transcriptions_timestamp ON transcriptions(timestamp);
        CREATE INDEX IF NOT EXISTS idx_transcriptions_source_app ON transcriptions(source_app);
        CREATE INDEX IF NOT EXISTS idx_transcriptions_bookmarked ON transcriptions(is_bookmarked);

        CREATE VIRTUAL TABLE IF NOT EXISTS transcriptions_fts USING fts5(
            text,
            content=transcriptions,
            content_rowid=rowid
        );

        CREATE TRIGGER IF NOT EXISTS transcriptions_ai AFTER INSERT ON transcriptions BEGIN
            INSERT INTO transcriptions_fts(rowid, text) VALUES (new.rowid, new.text);
        END;
        CREATE TRIGGER IF NOT EXISTS transcriptions_ad AFTER DELETE ON transcriptions BEGIN
            INSERT INTO transcriptions_fts(transcriptions_fts, rowid, text) VALUES('delete', old.rowid, old.text);
        END;
        CREATE TRIGGER IF NOT EXISTS transcriptions_au AFTER UPDATE ON transcriptions BEGIN
            INSERT INTO transcriptions_fts(transcriptions_fts, rowid, text) VALUES('delete', old.rowid, old.text);
            INSERT INTO transcriptions_fts(rowid, text) VALUES (new.rowid, new.text);
        END;

        CREATE TABLE IF NOT EXISTS api_keys (
            provider TEXT PRIMARY KEY,
            encrypted_key TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS custom_prompts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            prompt TEXT NOT NULL,
            app_pattern TEXT DEFAULT NULL
        );",
    )
    .map_err(|e| format!("Failed to initialize database: {e}"))?;

    // Migrate data from old database files if they exist
    migrate_old_databases(&conn)?;

    Ok(conn)
}

fn migrate_old_databases(conn: &Connection) -> Result<(), String> {
    let dir = data_dir()?;
    let old_files = [
        ("models.db", migrate_models),
        ("dictionary.db", migrate_dictionary),
        ("history.db", migrate_history),
        ("ai.db", migrate_ai),
    ];

    for (filename, migrate_fn) in &old_files {
        let old_path = dir.join(filename);
        if old_path.exists() {
            match migrate_fn(conn, &old_path) {
                Ok(()) => {
                    // Delete old file after successful migration
                    if let Err(e) = std::fs::remove_file(&old_path) {
                        eprintln!("Warning: failed to delete old {filename}: {e}");
                    }
                }
                Err(e) => {
                    eprintln!("Warning: failed to migrate {filename}: {e}");
                }
            }
        }
    }

    Ok(())
}

fn migrate_models(conn: &Connection, old_path: &PathBuf) -> Result<(), String> {
    let old_conn =
        Connection::open(old_path).map_err(|e| format!("Failed to open old models.db: {e}"))?;
    let mut stmt = old_conn
        .prepare("SELECT name, filename, size_bytes, path, version, downloaded_at FROM models")
        .map_err(|e| format!("Failed to read old models: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .map_err(|e| format!("Failed to query old models: {e}"))?;

    for row in rows {
        let (name, filename, size_bytes, path, version, downloaded_at) =
            row.map_err(|e| format!("Row error: {e}"))?;
        conn.execute(
            "INSERT OR IGNORE INTO models (name, filename, size_bytes, path, version, downloaded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![name, filename, size_bytes, path, version, downloaded_at],
        )
        .map_err(|e| format!("Failed to migrate model: {e}"))?;
    }
    Ok(())
}

fn migrate_dictionary(conn: &Connection, old_path: &PathBuf) -> Result<(), String> {
    let old_conn = Connection::open(old_path)
        .map_err(|e| format!("Failed to open old dictionary.db: {e}"))?;
    let mut stmt = old_conn
        .prepare("SELECT from_text, to_text, case_sensitive FROM dictionary")
        .map_err(|e| format!("Failed to read old dictionary: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i32>(2)?,
            ))
        })
        .map_err(|e| format!("Failed to query old dictionary: {e}"))?;

    for row in rows {
        let (from_text, to_text, case_sensitive) =
            row.map_err(|e| format!("Row error: {e}"))?;
        conn.execute(
            "INSERT INTO dictionary (from_text, to_text, case_sensitive) VALUES (?1, ?2, ?3)",
            rusqlite::params![from_text, to_text, case_sensitive],
        )
        .map_err(|e| format!("Failed to migrate dictionary entry: {e}"))?;
    }
    Ok(())
}

fn migrate_history(conn: &Connection, old_path: &PathBuf) -> Result<(), String> {
    let old_conn = Connection::open(old_path)
        .map_err(|e| format!("Failed to open old history.db: {e}"))?;
    let mut stmt = old_conn
        .prepare("SELECT id, text, raw_text, timestamp, duration_ms, source_app, language, model_used, is_bookmarked FROM transcriptions")
        .map_err(|e| format!("Failed to read old transcriptions: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<String>>(7)?,
                row.get::<_, i32>(8)?,
            ))
        })
        .map_err(|e| format!("Failed to query old transcriptions: {e}"))?;

    for row in rows {
        let (id, text, raw_text, timestamp, duration_ms, source_app, language, model_used, is_bookmarked) =
            row.map_err(|e| format!("Row error: {e}"))?;
        conn.execute(
            "INSERT OR IGNORE INTO transcriptions (id, text, raw_text, timestamp, duration_ms, source_app, language, model_used, is_bookmarked)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![id, text, raw_text, timestamp, duration_ms, source_app, language, model_used, is_bookmarked],
        )
        .map_err(|e| format!("Failed to migrate transcription: {e}"))?;
    }
    Ok(())
}

fn migrate_ai(conn: &Connection, old_path: &PathBuf) -> Result<(), String> {
    let old_conn =
        Connection::open(old_path).map_err(|e| format!("Failed to open old ai.db: {e}"))?;

    // Migrate api_keys
    let mut stmt = old_conn
        .prepare("SELECT provider, encrypted_key FROM api_keys")
        .map_err(|e| format!("Failed to read old api_keys: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| format!("Failed to query old api_keys: {e}"))?;

    for row in rows {
        let (provider, encrypted_key) = row.map_err(|e| format!("Row error: {e}"))?;
        conn.execute(
            "INSERT OR IGNORE INTO api_keys (provider, encrypted_key) VALUES (?1, ?2)",
            rusqlite::params![provider, encrypted_key],
        )
        .map_err(|e| format!("Failed to migrate api key: {e}"))?;
    }

    // Migrate custom_prompts
    let mut stmt = old_conn
        .prepare("SELECT name, prompt, app_pattern FROM custom_prompts")
        .map_err(|e| format!("Failed to read old custom_prompts: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|e| format!("Failed to query old custom_prompts: {e}"))?;

    for row in rows {
        let (name, prompt, app_pattern) = row.map_err(|e| format!("Row error: {e}"))?;
        conn.execute(
            "INSERT INTO custom_prompts (name, prompt, app_pattern) VALUES (?1, ?2, ?3)",
            rusqlite::params![name, prompt, app_pattern],
        )
        .map_err(|e| format!("Failed to migrate custom prompt: {e}"))?;
    }

    Ok(())
}
