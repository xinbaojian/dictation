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
    /// 是否在每次启动时自动检查新版本；关闭后只能手动点「检查更新」
    pub auto_check_updates: bool,
    /// 笔画复习页翻到每个词时是否自动朗读；默认关，页内与设置页共用这一个开关
    pub stroke_auto_speak: bool,
}

impl Default for SettingsDto {
    fn default() -> Self {
        Self {
            mask_seconds: 3,
            streak_to_master: 2,
            word_font_size: 96,
            auto_check_updates: true,
            stroke_auto_speak: false,
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
        auto_check_updates: input.auto_check_updates,
        stroke_auto_speak: input.stroke_auto_speak,
    };
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![KEY, serde_json::to_string(&value)?],
    )?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    /// 加了这个字段之前存下的设置 JSON 没有它：反序列化必须按默认「开」补齐
    /// （结构体上的 serde(default) 回落到 Default 实现），否则升级后老用户被静默改成「关」。
    #[test]
    fn legacy_settings_without_auto_check_defaults_on() {
        let conn = db::open_memory();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)",
            params![KEY, r#"{"maskSeconds":3,"streakToMaster":2,"wordFontSize":96}"#],
        )
        .unwrap();
        assert!(load(&conn).unwrap().auto_check_updates);
    }

    #[test]
    fn auto_check_updates_round_trip() {
        let conn = db::open_memory();
        assert!(SettingsDto::default().auto_check_updates, "默认开启");

        let dto = SettingsDto {
            auto_check_updates: false,
            ..Default::default()
        };
        let saved = save(&conn, &dto).unwrap();
        assert!(!saved.auto_check_updates);
        assert!(!load(&conn).unwrap().auto_check_updates, "关闭要能存住");
    }

    /// 加了这个字段之前存下的设置 JSON 没有它：反序列化必须按默认「关」补齐。
    /// （自动朗读曾存前端 localStorage 且默认开，本字段上线后统一改为默认关，老用户需显式开启）
    #[test]
    fn legacy_settings_without_stroke_auto_speak_defaults_off() {
        let conn = db::open_memory();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)",
            params![KEY, r#"{"maskSeconds":3,"streakToMaster":2,"wordFontSize":96,"autoCheckUpdates":true}"#],
        )
        .unwrap();
        assert!(!load(&conn).unwrap().stroke_auto_speak);
    }

    #[test]
    fn stroke_auto_speak_round_trip() {
        let conn = db::open_memory();
        assert!(!SettingsDto::default().stroke_auto_speak, "默认关闭");

        let dto = SettingsDto {
            stroke_auto_speak: true,
            ..Default::default()
        };
        let saved = save(&conn, &dto).unwrap();
        assert!(saved.stroke_auto_speak);
        assert!(load(&conn).unwrap().stroke_auto_speak, "开启要能存住");
    }
}
