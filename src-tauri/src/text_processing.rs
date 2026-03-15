use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictionaryEntry {
    pub id: i64,
    pub from_text: String,
    pub to_text: String,
    pub case_sensitive: bool,
}

fn db_path() -> PathBuf {
    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("outspoken");
    std::fs::create_dir_all(&data_dir).ok();
    data_dir.join("dictionary.db")
}

fn open_db() -> Result<Connection, String> {
    let path = db_path();
    let conn =
        Connection::open(&path).map_err(|e| format!("Failed to open dictionary db: {e}"))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS dictionary (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            from_text TEXT NOT NULL,
            to_text TEXT NOT NULL,
            case_sensitive INTEGER NOT NULL DEFAULT 0
        );",
    )
    .map_err(|e| format!("Failed to create dictionary table: {e}"))?;
    Ok(conn)
}

pub fn add_entry(from_text: &str, to_text: &str, case_sensitive: bool) -> Result<DictionaryEntry, String> {
    let conn = open_db()?;
    conn.execute(
        "INSERT INTO dictionary (from_text, to_text, case_sensitive) VALUES (?1, ?2, ?3)",
        params![from_text, to_text, case_sensitive as i32],
    )
    .map_err(|e| format!("Failed to add dictionary entry: {e}"))?;
    let id = conn.last_insert_rowid();
    Ok(DictionaryEntry {
        id,
        from_text: from_text.to_string(),
        to_text: to_text.to_string(),
        case_sensitive,
    })
}

pub fn remove_entry(id: i64) -> Result<(), String> {
    let conn = open_db()?;
    let affected = conn
        .execute("DELETE FROM dictionary WHERE id = ?1", params![id])
        .map_err(|e| format!("Failed to remove dictionary entry: {e}"))?;
    if affected == 0 {
        return Err(format!("No dictionary entry found with id: {id}"));
    }
    Ok(())
}

pub fn list_entries() -> Result<Vec<DictionaryEntry>, String> {
    let conn = open_db()?;
    let mut stmt = conn
        .prepare("SELECT id, from_text, to_text, case_sensitive FROM dictionary ORDER BY id")
        .map_err(|e| format!("Failed to query dictionary: {e}"))?;
    let entries = stmt
        .query_map([], |row| {
            Ok(DictionaryEntry {
                id: row.get(0)?,
                from_text: row.get(1)?,
                to_text: row.get(2)?,
                case_sensitive: row.get::<_, i32>(3)? != 0,
            })
        })
        .map_err(|e| format!("Failed to query dictionary: {e}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to collect dictionary entries: {e}"))?;
    Ok(entries)
}

/// Default filler words/phrases to remove.
const FILLER_PHRASES: &[&str] = &[
    "you know",
    "i mean",
    "sort of",
    "kind of",
    "basically",
    "actually",
    "literally",
];

const FILLER_WORDS: &[&str] = &["um", "uh", "er", "ah"];

/// Remove filler words from text. Context-aware: keeps "like" when used as a verb.
pub fn remove_filler_words(text: &str) -> String {
    let mut result = text.to_string();

    // Remove multi-word filler phrases first (case-insensitive).
    for phrase in FILLER_PHRASES {
        result = remove_phrase_case_insensitive(&result, phrase);
    }

    // Remove single filler words that stand alone (bounded by word boundaries).
    for word in FILLER_WORDS {
        result = remove_standalone_word(&result, word);
    }

    // Context-aware "like" removal: remove "like" only when used as a filler,
    // not as a verb (e.g., "I like this"). Filler "like" typically appears:
    // - at sentence start: "Like, I was thinking..."
    // - mid-sentence as pause: "I was, like, thinking..."
    // We keep "like" when preceded by a subject pronoun or noun pattern suggesting verb usage.
    result = remove_filler_like(&result);

    // Clean up extra whitespace and fix punctuation spacing.
    cleanup_whitespace(&result)
}

/// Apply personal dictionary replacements to text.
pub fn apply_dictionary(text: &str, entries: &[DictionaryEntry]) -> String {
    let mut result = text.to_string();
    for entry in entries {
        if entry.case_sensitive {
            result = replace_whole_phrase(&result, &entry.from_text, &entry.to_text, true);
        } else {
            result = replace_whole_phrase(&result, &entry.from_text, &entry.to_text, false);
        }
    }
    result
}

/// Full post-processing pipeline.
pub fn process_text(
    text: &str,
    strip_fillers: bool,
    dictionary_entries: &[DictionaryEntry],
) -> String {
    let mut result = text.to_string();
    if strip_fillers {
        result = remove_filler_words(&result);
    }
    if !dictionary_entries.is_empty() {
        result = apply_dictionary(&result, dictionary_entries);
    }
    result
}

// --- Helper functions ---

fn is_word_boundary(c: char) -> bool {
    !c.is_alphanumeric() && c != '\''
}

fn remove_phrase_case_insensitive(text: &str, phrase: &str) -> String {
    let lower = text.to_lowercase();
    let phrase_lower = phrase.to_lowercase();
    let phrase_len = phrase.len();
    let mut result = String::with_capacity(text.len());
    let mut i = 0;
    let text_bytes = text.as_bytes();

    while i < text.len() {
        if i + phrase_len <= text.len() && lower[i..i + phrase_len] == phrase_lower {
            // Check word boundaries.
            let before_ok =
                i == 0 || is_word_boundary(text[..i].chars().last().unwrap());
            let after_ok = i + phrase_len == text.len()
                || is_word_boundary(text_bytes[i + phrase_len] as char);
            if before_ok && after_ok {
                // Skip optional trailing comma/space.
                let mut skip_to = i + phrase_len;
                if skip_to < text.len() && text_bytes[skip_to] == b',' {
                    skip_to += 1;
                }
                while skip_to < text.len() && text_bytes[skip_to] == b' ' {
                    skip_to += 1;
                }
                i = skip_to;
                continue;
            }
        }
        result.push(text[i..].chars().next().unwrap());
        i += text[i..].chars().next().unwrap().len_utf8();
    }
    result
}

fn remove_standalone_word(text: &str, word: &str) -> String {
    let lower = text.to_lowercase();
    let word_lower = word.to_lowercase();
    let word_len = word.len();
    let mut result = String::with_capacity(text.len());
    let mut i = 0;
    let text_bytes = text.as_bytes();

    while i < text.len() {
        if i + word_len <= text.len() && lower[i..i + word_len] == word_lower {
            let before_ok =
                i == 0 || is_word_boundary(text[..i].chars().last().unwrap());
            let after_ok = i + word_len == text.len()
                || is_word_boundary(text_bytes[i + word_len] as char);
            if before_ok && after_ok {
                let mut skip_to = i + word_len;
                if skip_to < text.len() && text_bytes[skip_to] == b',' {
                    skip_to += 1;
                }
                while skip_to < text.len() && text_bytes[skip_to] == b' ' {
                    skip_to += 1;
                }
                i = skip_to;
                continue;
            }
        }
        result.push(text[i..].chars().next().unwrap());
        i += text[i..].chars().next().unwrap().len_utf8();
    }
    result
}

fn remove_filler_like(text: &str) -> String {
    let lower = text.to_lowercase();
    let mut result = String::with_capacity(text.len());
    let mut i = 0;
    let text_bytes = text.as_bytes();

    while i < text.len() {
        if i + 4 <= text.len() && &lower[i..i + 4] == "like" {
            let before_ok =
                i == 0 || is_word_boundary(text[..i].chars().last().unwrap());
            let after_ok =
                i + 4 == text.len() || is_word_boundary(text_bytes[i + 4] as char);

            if before_ok && after_ok {
                // Check if "like" is a verb: preceded by subject pronoun.
                let preceding = text[..i].trim_end().to_lowercase();
                let is_verb = preceding.ends_with("i")
                    || preceding.ends_with("we")
                    || preceding.ends_with("they")
                    || preceding.ends_with("you")
                    || preceding.ends_with("would")
                    || preceding.ends_with("don't")
                    || preceding.ends_with("didn't")
                    || preceding.ends_with("really")
                    || preceding.ends_with("also")
                    || preceding.ends_with("to");

                if !is_verb {
                    let mut skip_to = i + 4;
                    if skip_to < text.len() && text_bytes[skip_to] == b',' {
                        skip_to += 1;
                    }
                    while skip_to < text.len() && text_bytes[skip_to] == b' ' {
                        skip_to += 1;
                    }
                    i = skip_to;
                    continue;
                }
            }
        }
        result.push(text[i..].chars().next().unwrap());
        i += text[i..].chars().next().unwrap().len_utf8();
    }
    result
}

fn replace_whole_phrase(text: &str, from: &str, to: &str, case_sensitive: bool) -> String {
    let search_text = if case_sensitive {
        text.to_string()
    } else {
        text.to_lowercase()
    };
    let search_from = if case_sensitive {
        from.to_string()
    } else {
        from.to_lowercase()
    };
    let from_len = from.len();
    let mut result = String::with_capacity(text.len());
    let mut i = 0;

    while i < text.len() {
        if i + from_len <= text.len() && search_text[i..i + from_len] == search_from {
            let before_ok =
                i == 0 || is_word_boundary(text[..i].chars().last().unwrap());
            let after_ok = i + from_len == text.len()
                || is_word_boundary(text.as_bytes()[i + from_len] as char);
            if before_ok && after_ok {
                result.push_str(to);
                i += from_len;
                continue;
            }
        }
        result.push(text[i..].chars().next().unwrap());
        i += text[i..].chars().next().unwrap().len_utf8();
    }
    result
}

fn cleanup_whitespace(text: &str) -> String {
    // Collapse multiple spaces into one.
    let mut result = String::with_capacity(text.len());
    let mut prev_space = false;
    for c in text.chars() {
        if c == ' ' {
            if !prev_space {
                result.push(' ');
            }
            prev_space = true;
        } else {
            prev_space = false;
            result.push(c);
        }
    }
    // Trim leading/trailing spaces and fix leading space after sentence-start punctuation.
    let result = result.trim().to_string();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filler_removal_basic() {
        let input = "Um so I was uh thinking";
        let output = remove_filler_words(input);
        assert_eq!(output, "so I was thinking");
    }

    #[test]
    fn test_filler_removal_preserves_like_verb() {
        let input = "I like this idea";
        let output = remove_filler_words(input);
        assert_eq!(output, "I like this idea");
    }

    #[test]
    fn test_filler_removal_removes_filler_like() {
        let input = "It was, like, really good";
        let output = remove_filler_words(input);
        assert_eq!(output, "It was, really good");
    }

    #[test]
    fn test_filler_removal_multi_word() {
        let input = "I mean, basically it works you know";
        let output = remove_filler_words(input);
        assert_eq!(output, "it works");
    }

    #[test]
    fn test_dictionary_case_insensitive() {
        let entries = vec![DictionaryEntry {
            id: 1,
            from_text: "eye phone".into(),
            to_text: "iPhone".into(),
            case_sensitive: false,
        }];
        let result = apply_dictionary("I got an Eye Phone today", &entries);
        assert_eq!(result, "I got an iPhone today");
    }

    #[test]
    fn test_dictionary_case_sensitive() {
        let entries = vec![DictionaryEntry {
            id: 1,
            from_text: "rust".into(),
            to_text: "Rust".into(),
            case_sensitive: true,
        }];
        assert_eq!(apply_dictionary("I write rust code", &entries), "I write Rust code");
        assert_eq!(apply_dictionary("I write Rust code", &entries), "I write Rust code");
    }

    #[test]
    fn test_process_text_combined() {
        let entries = vec![DictionaryEntry {
            id: 1,
            from_text: "eye phone".into(),
            to_text: "iPhone".into(),
            case_sensitive: false,
        }];
        let input = "Um so I got an eye phone";
        let output = process_text(input, true, &entries);
        assert_eq!(output, "so I got an iPhone");
    }
}
