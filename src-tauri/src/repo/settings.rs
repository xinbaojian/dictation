use crate::error::AppResult;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

const KEY: &str = "app";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SettingsDto {
    /// 词语显示后自动重新遮罩的秒数，0 表示不自动隐藏
    pub mask_seconds: i64,
    /// 错题本连续答对多少次算学会
    pub streak_to_master: i64,
    /// 默写页词语字号（px）
    pub word_font_size: i64,
    /// 随机抽词时优先让下一个词和上一个词有相同的字（连字）
    pub chain_by_char: bool,
}

impl Default for SettingsDto {
    fn default() -> Self {
        Self {
            mask_seconds: 3,
            streak_to_master: 2,
            word_font_size: 96,
            chain_by_char: false,
        }
    }
}

pub fn load(conn: &Connection) -> AppResult<SettingsDto> {
    let raw: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = ?1", params![KEY], |r| r.get(0))
        .optional()?;
    Ok(raw
        .and_then(|s| serde_json::from_str::<SettingsDto>(&s).ok())
        .unwrap_or_default())
}

pub fn save(conn: &Connection, input: &SettingsDto) -> AppResult<SettingsDto> {
    let value = SettingsDto {
        mask_seconds: input.mask_seconds.clamp(0, 15),
        streak_to_master: input.streak_to_master.clamp(1, 5),
        word_font_size: input.word_font_size.clamp(48, 200),
        chain_by_char: input.chain_by_char,
    };
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![KEY, serde_json::to_string(&value)?],
    )?;
    Ok(value)
}
