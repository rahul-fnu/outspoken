use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::db;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Tone {
    Casual,
    Professional,
    General,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PunctuationLevel {
    Minimal,
    Full,
    Standard,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormatProfile {
    pub name: String,
    pub tone: Tone,
    pub punctuation_level: PunctuationLevel,
    pub line_breaking: bool,
}

fn messaging_profile() -> FormatProfile {
    FormatProfile {
        name: "messaging".to_string(),
        tone: Tone::Casual,
        punctuation_level: PunctuationLevel::Minimal,
        line_breaking: false,
    }
}

fn email_profile() -> FormatProfile {
    FormatProfile {
        name: "email".to_string(),
        tone: Tone::Professional,
        punctuation_level: PunctuationLevel::Full,
        line_breaking: true,
    }
}

fn general_profile() -> FormatProfile {
    FormatProfile {
        name: "general".to_string(),
        tone: Tone::General,
        punctuation_level: PunctuationLevel::Standard,
        line_breaking: false,
    }
}

const MESSAGING_APPS: &[&str] = &[
    "com.tinyspeck.slackmacgap",
    "slack",
    "com.hnc.discord",
    "discord",
    "com.apple.mobilephone",
    "com.apple.imessage",
    "com.apple.messages",
    "imessage",
    "net.whatsapp.whatsapp",
    "whatsapp",
    "org.telegram.desktop",
    "telegram",
];

const EMAIL_APPS: &[&str] = &[
    "com.apple.mail",
    "mail",
    "com.google.gmail",
    "gmail",
    "com.microsoft.outlook",
    "outlook",
];

fn lookup_custom_profile(bundle_id_or_name: &str) -> Option<String> {
    let conn = db::open_db().ok()?;
    let mut stmt = conn
        .prepare("SELECT profile_name FROM app_format_profiles WHERE app_identifier = ?1")
        .ok()?;
    stmt.query_row(params![bundle_id_or_name], |row| row.get::<_, String>(0))
        .ok()
}

fn profile_by_name(name: &str) -> FormatProfile {
    match name {
        "messaging" => messaging_profile(),
        "email" => email_profile(),
        _ => general_profile(),
    }
}

pub fn get_profile_for_app(bundle_id_or_name: &str) -> FormatProfile {
    let lower = bundle_id_or_name.to_lowercase();

    if let Some(name) = lookup_custom_profile(&lower) {
        return profile_by_name(&name);
    }

    for app in MESSAGING_APPS {
        if lower == *app {
            return messaging_profile();
        }
    }

    for app in EMAIL_APPS {
        if lower == *app {
            return email_profile();
        }
    }

    general_profile()
}

pub fn apply_format(text: &str, profile: &FormatProfile) -> String {
    match profile.tone {
        Tone::Casual => apply_casual(text),
        Tone::Professional => apply_professional(text),
        Tone::General => text.to_string(),
    }
}

fn apply_casual(text: &str) -> String {
    let mut result = text.to_string();
    // Remove trailing period
    if result.ends_with('.') {
        result.pop();
    }
    // Lowercase the first character
    lowercase_first(&result)
}

fn lowercase_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => {
            let lower: String = c.to_lowercase().collect();
            lower + chars.as_str()
        }
    }
}

fn apply_professional(text: &str) -> String {
    let mut result = capitalize_first(text);
    // Ensure text ends with a period if it doesn't end with punctuation
    if !result.is_empty() {
        let last = result.chars().last().unwrap();
        if !matches!(last, '.' | '!' | '?') {
            result.push('.');
        }
    }
    result
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().to_string() + chars.as_str(),
    }
}

pub fn list_profiles() -> Vec<FormatProfile> {
    vec![messaging_profile(), email_profile(), general_profile()]
}

pub fn set_app_profile(app_identifier: &str, profile_name: &str) -> Result<(), String> {
    let conn = db::open_db()?;
    conn.execute(
        "INSERT INTO app_format_profiles (app_identifier, profile_name) VALUES (?1, ?2)
         ON CONFLICT(app_identifier) DO UPDATE SET profile_name = excluded.profile_name",
        params![app_identifier.to_lowercase(), profile_name],
    )
    .map_err(|e| format!("Failed to set app profile: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_casual_format() {
        let profile = messaging_profile();
        assert_eq!(apply_format("Hello how are you.", &profile), "hello how are you");
    }

    #[test]
    fn test_professional_format() {
        let profile = email_profile();
        assert_eq!(apply_format("hello", &profile), "Hello.");
    }

    #[test]
    fn test_general_format() {
        let profile = general_profile();
        assert_eq!(apply_format("Hello world", &profile), "Hello world");
    }

    #[test]
    fn test_professional_keeps_existing_punctuation() {
        let profile = email_profile();
        assert_eq!(apply_format("Hello!", &profile), "Hello!");
        assert_eq!(apply_format("Hello?", &profile), "Hello?");
        assert_eq!(apply_format("Hello.", &profile), "Hello.");
    }

    #[test]
    fn test_list_profiles() {
        let profiles = list_profiles();
        assert_eq!(profiles.len(), 3);
        assert_eq!(profiles[0].name, "messaging");
        assert_eq!(profiles[1].name, "email");
        assert_eq!(profiles[2].name, "general");
    }
}
