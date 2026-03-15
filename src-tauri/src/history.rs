use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::db;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transcription {
    pub id: String,
    pub text: String,
    pub raw_text: String,
    pub timestamp: i64,
    pub duration_ms: Option<i64>,
    pub source_app: Option<String>,
    pub language: Option<String>,
    pub model_used: Option<String>,
    pub is_bookmarked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveTranscriptionInput {
    pub text: String,
    pub raw_text: Option<String>,
    pub duration_ms: Option<i64>,
    pub source_app: Option<String>,
    pub language: Option<String>,
    pub model_used: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchFilters {
    pub query: Option<String>,
    pub source_app: Option<String>,
    pub date_from: Option<i64>,
    pub date_to: Option<i64>,
    pub bookmarked_only: Option<bool>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryStats {
    pub total_count: i64,
    pub source_apps: Vec<String>,
}

fn open_db() -> Result<rusqlite::Connection, String> {
    db::open_db()
}

fn generate_id() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let random: u32 = rand::random();
    format!("{ts:x}-{random:08x}")
}

fn now_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn save_transcription(input: &SaveTranscriptionInput) -> Result<Transcription, String> {
    let conn = open_db()?;
    let id = generate_id();
    let timestamp = now_timestamp();
    let raw_text = input.raw_text.as_deref().unwrap_or(&input.text);

    conn.execute(
        "INSERT INTO transcriptions (id, text, raw_text, timestamp, duration_ms, source_app, language, model_used, is_bookmarked)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0)",
        params![
            id,
            input.text,
            raw_text,
            timestamp,
            input.duration_ms,
            input.source_app,
            input.language,
            input.model_used,
        ],
    )
    .map_err(|e| format!("Failed to save transcription: {e}"))?;

    Ok(Transcription {
        id,
        text: input.text.clone(),
        raw_text: raw_text.to_string(),
        timestamp,
        duration_ms: input.duration_ms,
        source_app: input.source_app.clone(),
        language: input.language.clone(),
        model_used: input.model_used.clone(),
        is_bookmarked: false,
    })
}

pub fn search_history(filters: &SearchFilters) -> Result<Vec<Transcription>, String> {
    let conn = open_db()?;

    let use_fts = filters
        .query
        .as_ref()
        .is_some_and(|q| !q.trim().is_empty());

    let mut sql = if use_fts {
        // Join with FTS for full-text search
        String::from(
            "SELECT t.id, t.text, t.raw_text, t.timestamp, t.duration_ms, t.source_app, t.language, t.model_used, t.is_bookmarked
             FROM transcriptions t
             JOIN transcriptions_fts fts ON t.rowid = fts.rowid
             WHERE transcriptions_fts MATCH ?",
        )
    } else {
        String::from(
            "SELECT id, text, raw_text, timestamp, duration_ms, source_app, language, model_used, is_bookmarked
             FROM transcriptions WHERE 1=1",
        )
    };

    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if use_fts {
        // FTS5 query: wrap each word with * for prefix matching
        let query = filters.query.as_ref().unwrap();
        let fts_query: String = query
            .split_whitespace()
            .map(|w| format!("\"{w}\"*"))
            .collect::<Vec<_>>()
            .join(" ");
        param_values.push(Box::new(fts_query));
    }

    if let Some(ref app) = filters.source_app {
        if !app.is_empty() {
            sql.push_str(" AND source_app = ?");
            param_values.push(Box::new(app.clone()));
        }
    }
    if let Some(from) = filters.date_from {
        sql.push_str(" AND timestamp >= ?");
        param_values.push(Box::new(from));
    }
    if let Some(to) = filters.date_to {
        sql.push_str(" AND timestamp <= ?");
        param_values.push(Box::new(to));
    }
    if filters.bookmarked_only == Some(true) {
        sql.push_str(" AND is_bookmarked = 1");
    }

    if use_fts {
        sql.push_str(" ORDER BY rank");
    } else {
        sql.push_str(" ORDER BY timestamp DESC");
    }

    let limit = filters.limit.unwrap_or(100);
    let offset = filters.offset.unwrap_or(0);
    sql.push_str(" LIMIT ? OFFSET ?");
    param_values.push(Box::new(limit));
    param_values.push(Box::new(offset));

    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("Failed to prepare query: {e}"))?;

    let entries = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(Transcription {
                id: row.get(0)?,
                text: row.get(1)?,
                raw_text: row.get(2)?,
                timestamp: row.get(3)?,
                duration_ms: row.get(4)?,
                source_app: row.get(5)?,
                language: row.get(6)?,
                model_used: row.get(7)?,
                is_bookmarked: row.get::<_, i32>(8)? != 0,
            })
        })
        .map_err(|e| format!("Failed to query history: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to collect history entries: {e}"))?;

    Ok(entries)
}

pub fn delete_transcription(id: &str) -> Result<(), String> {
    let conn = open_db()?;
    let affected = conn
        .execute("DELETE FROM transcriptions WHERE id = ?1", params![id])
        .map_err(|e| format!("Failed to delete transcription: {e}"))?;
    if affected == 0 {
        return Err(format!("No transcription found with id: {id}"));
    }
    Ok(())
}

pub fn clear_history() -> Result<(), String> {
    let conn = open_db()?;
    conn.execute_batch("DELETE FROM transcriptions;")
        .map_err(|e| format!("Failed to clear history: {e}"))?;
    Ok(())
}

pub fn toggle_bookmark(id: &str) -> Result<bool, String> {
    let conn = open_db()?;
    let current: i32 = conn
        .query_row(
            "SELECT is_bookmarked FROM transcriptions WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .map_err(|e| format!("Transcription not found: {e}"))?;

    let new_val = if current == 0 { 1 } else { 0 };
    conn.execute(
        "UPDATE transcriptions SET is_bookmarked = ?1 WHERE id = ?2",
        params![new_val, id],
    )
    .map_err(|e| format!("Failed to toggle bookmark: {e}"))?;

    Ok(new_val == 1)
}

pub fn get_stats() -> Result<HistoryStats, String> {
    let conn = open_db()?;
    let total_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM transcriptions", [], |row| row.get(0))
        .map_err(|e| format!("Failed to count history: {e}"))?;

    let mut stmt = conn
        .prepare("SELECT DISTINCT source_app FROM transcriptions WHERE source_app IS NOT NULL AND source_app != '' ORDER BY source_app")
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

pub fn export_history(filters: &SearchFilters, format: &str) -> Result<String, String> {
    // Use a large limit for exports
    let mut export_filters = filters.clone();
    if export_filters.limit.is_none() {
        export_filters.limit = Some(100_000);
    }
    let entries = search_history(&export_filters)?;

    match format {
        "json" => serde_json::to_string_pretty(&entries)
            .map_err(|e| format!("Failed to serialize to JSON: {e}")),
        "csv" => {
            let mut csv = String::from(
                "id,text,raw_text,timestamp,duration_ms,source_app,language,model_used,is_bookmarked\n",
            );
            for entry in &entries {
                csv.push_str(&format!(
                    "{},\"{}\",\"{}\",{},{},{},{},{},{}\n",
                    csv_escape(&entry.id),
                    csv_escape(&entry.text),
                    csv_escape(&entry.raw_text),
                    entry.timestamp,
                    entry.duration_ms.map_or(String::new(), |v| v.to_string()),
                    csv_escape(entry.source_app.as_deref().unwrap_or("")),
                    csv_escape(entry.language.as_deref().unwrap_or("")),
                    csv_escape(entry.model_used.as_deref().unwrap_or("")),
                    if entry.is_bookmarked { 1 } else { 0 },
                ));
            }
            Ok(csv)
        }
        "txt" => {
            let mut txt = String::new();
            for entry in &entries {
                let datetime = format_timestamp(entry.timestamp);
                txt.push_str(&format!("[{}]", datetime));
                if let Some(ref app) = entry.source_app {
                    if !app.is_empty() {
                        txt.push_str(&format!(" ({})", app));
                    }
                }
                if let Some(ms) = entry.duration_ms {
                    txt.push_str(&format!(" {}s", ms / 1000));
                }
                if entry.is_bookmarked {
                    txt.push_str(" [bookmarked]");
                }
                txt.push('\n');
                txt.push_str(&entry.text);
                txt.push_str("\n\n");
            }
            Ok(txt)
        }
        _ => Err(format!("Unsupported export format: {format}")),
    }
}

fn csv_escape(s: &str) -> String {
    s.replace('"', "\"\"")
}

fn format_timestamp(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| ts.to_string())
}
