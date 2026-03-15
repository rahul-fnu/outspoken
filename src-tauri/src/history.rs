use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: i64,
    pub text: String,
    pub source_app: String,
    pub duration_secs: f64,
    pub language: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryQuery {
    pub search: Option<String>,
    pub source_app: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryStats {
    pub total_count: i64,
    pub source_apps: Vec<String>,
}

fn db_path() -> PathBuf {
    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("outspoken");
    std::fs::create_dir_all(&data_dir).ok();
    data_dir.join("history.db")
}

fn open_db() -> Result<Connection, String> {
    let path = db_path();
    let conn = Connection::open(&path).map_err(|e| format!("Failed to open history db: {e}"))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            text TEXT NOT NULL,
            source_app TEXT NOT NULL DEFAULT '',
            duration_secs REAL NOT NULL DEFAULT 0.0,
            language TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_history_created_at ON history(created_at);
        CREATE INDEX IF NOT EXISTS idx_history_source_app ON history(source_app);",
    )
    .map_err(|e| format!("Failed to create history table: {e}"))?;
    Ok(conn)
}

pub fn save_entry(
    text: &str,
    source_app: &str,
    duration_secs: f64,
    language: &str,
) -> Result<HistoryEntry, String> {
    let conn = open_db()?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO history (text, source_app, duration_secs, language, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![text, source_app, duration_secs, language, now],
    )
    .map_err(|e| format!("Failed to save history entry: {e}"))?;

    let id = conn.last_insert_rowid();
    Ok(HistoryEntry {
        id,
        text: text.to_string(),
        source_app: source_app.to_string(),
        duration_secs,
        language: language.to_string(),
        created_at: now,
    })
}

pub fn query_entries(query: &HistoryQuery) -> Result<Vec<HistoryEntry>, String> {
    let conn = open_db()?;

    let mut sql = String::from(
        "SELECT id, text, source_app, duration_secs, language, created_at FROM history WHERE 1=1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(ref search) = query.search {
        if !search.is_empty() {
            sql.push_str(" AND text LIKE ?");
            param_values.push(Box::new(format!("%{search}%")));
        }
    }
    if let Some(ref app) = query.source_app {
        if !app.is_empty() {
            sql.push_str(" AND source_app = ?");
            param_values.push(Box::new(app.clone()));
        }
    }
    if let Some(ref from) = query.date_from {
        if !from.is_empty() {
            sql.push_str(" AND created_at >= ?");
            param_values.push(Box::new(from.clone()));
        }
    }
    if let Some(ref to) = query.date_to {
        if !to.is_empty() {
            sql.push_str(" AND created_at <= ?");
            param_values.push(Box::new(to.clone()));
        }
    }

    sql.push_str(" ORDER BY created_at DESC");

    let limit = query.limit.unwrap_or(100);
    let offset = query.offset.unwrap_or(0);
    sql.push_str(" LIMIT ? OFFSET ?");
    param_values.push(Box::new(limit));
    param_values.push(Box::new(offset));

    let params_refs: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("Failed to prepare query: {e}"))?;

    let entries = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(HistoryEntry {
                id: row.get(0)?,
                text: row.get(1)?,
                source_app: row.get(2)?,
                duration_secs: row.get(3)?,
                language: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .map_err(|e| format!("Failed to query history: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to collect history entries: {e}"))?;

    Ok(entries)
}

pub fn delete_entry(id: i64) -> Result<(), String> {
    let conn = open_db()?;
    let affected = conn
        .execute("DELETE FROM history WHERE id = ?1", params![id])
        .map_err(|e| format!("Failed to delete history entry: {e}"))?;
    if affected == 0 {
        return Err(format!("No history entry found with id: {id}"));
    }
    Ok(())
}

pub fn get_stats() -> Result<HistoryStats, String> {
    let conn = open_db()?;
    let total_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM history", [], |row| row.get(0))
        .map_err(|e| format!("Failed to count history: {e}"))?;

    let mut stmt = conn
        .prepare("SELECT DISTINCT source_app FROM history WHERE source_app != '' ORDER BY source_app")
        .map_err(|e| format!("Failed to query source apps: {e}"))?;
    let source_apps = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| format!("Failed to query source apps: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to collect source apps: {e}"))?;

    Ok(HistoryStats {
        total_count,
        source_apps,
    })
}

pub fn export_entries(query: &HistoryQuery, format: &str) -> Result<String, String> {
    let entries = query_entries(query)?;

    match format {
        "json" => serde_json::to_string_pretty(&entries)
            .map_err(|e| format!("Failed to serialize to JSON: {e}")),
        "csv" => {
            let mut csv = String::from("id,text,source_app,duration_secs,language,created_at\n");
            for entry in &entries {
                csv.push_str(&format!(
                    "{},\"{}\",\"{}\",{},{},{}\n",
                    entry.id,
                    entry.text.replace('"', "\"\""),
                    entry.source_app.replace('"', "\"\""),
                    entry.duration_secs,
                    entry.language,
                    entry.created_at,
                ));
            }
            Ok(csv)
        }
        "txt" => {
            let mut txt = String::new();
            for entry in &entries {
                txt.push_str(&format!(
                    "[{}] ({}, {}s, {})\n{}\n\n",
                    entry.created_at,
                    entry.source_app,
                    entry.duration_secs as i64,
                    entry.language,
                    entry.text,
                ));
            }
            Ok(txt)
        }
        _ => Err(format!("Unsupported export format: {format}")),
    }
}
