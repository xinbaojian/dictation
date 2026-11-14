//! 查字工具的 AI 字卡领域逻辑：prompt 构造 + LLM 输出校验（纯函数，不依赖 Tauri / DB）。
//! 与 ai_card.rs 同一套路，但对象是「任意单个汉字」（不要求在词表里），缓存键是字本身。
use serde::{Deserialize, Serialize};

use crate::domain::ai_card::{strip_code_fence, ChatMessage};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CharCardContent {
    /// 字义 / 来历小故事（讲给孩子听）
    pub meaning: String,
    /// 巧记口诀
    pub mnemonic: String,
    /// 含该字的常用词语 2~4 个
    pub words: Vec<String>,
    /// 例句 1~2 句，每句包含该字
    pub sentences: Vec<String>,
}

/// 一次字卡生成的上下文（查字工具没有错题信息，只有字本身）
pub struct CharContext {
    pub char: String,
}

const SYSTEM_PROMPT: &str = "你是给中国小学二年级孩子（7~8 岁）讲语文的老师，家长刚问你这个字怎么写、怎么记。请严格按以下要求输出：
1. meaning：用一两句话给孩子讲这个字的意思或来历，不超过 60 个字，比喻只用孩子熟悉的事物（动物、学校、家里、玩具、食物）。
2. mnemonic：一句能念出声的巧记口诀，不超过 40 个字，朗朗上口。
3. words：2~4 个含有这个字的常用词语，每个 2~4 个字。
4. sentences：1~2 个例句，每句不超过 40 个字，必须完整包含这个字，写二年级孩子的生活场景，用词不超纲。
5. 口诀和例句里不许出现这些词：形近字、音近字、部首、音序、笔顺、笔画、偏旁。所有内容都不要换行。
6. 只输出一个 JSON 对象，不要输出 JSON 以外的任何文字，格式示例：
{\"meaning\":\"碧就是青绿色的玉石，像天空和大海的颜色\",\"mnemonic\":\"王爷家的白玉石，青绿青绿真好看\",\"words\":[\"碧绿\",\"碧玉\",\"碧空\"],\"sentences\":[\"湖水碧绿得像一块玉石。\"]}";

/// 课堂术语黑名单：口诀和例句是孩子直接念的（与 ai_card 同一约束）
const TERMS: [&str; 7] = ["形近字", "音近字", "部首", "音序", "笔顺", "笔画", "偏旁"];

pub fn build_messages(ctx: &CharContext) -> Vec<ChatMessage> {
    vec![
        ChatMessage { role: "system".into(), content: SYSTEM_PROMPT.into() },
        ChatMessage {
            role: "user".into(),
            content: format!("字：{}", ctx.char),
        },
    ]
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

fn find_term(text: &str) -> Option<&'static str> {
    TERMS.iter().find(|t| text.contains(*t)).copied()
}

/// LLM 输出的唯一入库关卡：解析 → 规整 → 校验。文案要能回喂 LLM 也能直接给家长看。
pub fn validate(raw: &str, ch: &str) -> AppResult<CharCardContent> {
    let clean = strip_code_fence(raw);
    let mut c: CharCardContent = serde_json::from_str(clean)
        .map_err(|_| AppError::msg("AI 输出不是合法的 JSON（缺字段或格式不对）"))?;
    c.meaning = c.meaning.trim().to_string();
    c.mnemonic = c.mnemonic.trim().to_string();
    c.words = c.words.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
    c.sentences = c.sentences.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();

    // 字义（讲来历可能自然带部件说法，不查术语，只查长度与换行）
    if c.meaning.is_empty() {
        return Err(AppError::msg("字义不能为空"));
    }
    if c.meaning.contains('\n') {
        return Err(AppError::msg("字义里不能换行"));
    }
    if char_len(&c.meaning) > 60 {
        return Err(AppError::msg("字义太长了，请压到 60 个字以内"));
    }
    // 口诀
    if c.mnemonic.is_empty() {
        return Err(AppError::msg("口诀不能为空"));
    }
    if c.mnemonic.contains('\n') {
        return Err(AppError::msg("口诀里不能换行"));
    }
    if char_len(&c.mnemonic) > 40 {
        return Err(AppError::msg("口诀太长了，请压到 40 个字以内"));
    }
    if let Some(t) = find_term(&c.mnemonic) {
        return Err(AppError::msg(format!("口诀里用了课堂术语「{t}」，请换成孩子听得懂的说法")));
    }
    // 组词
    if c.words.len() < 2 || c.words.len() > 4 {
        return Err(AppError::msg("组词要 2~4 个"));
    }
    for w in &c.words {
        if w.contains('\n') || char_len(w) > 6 {
            return Err(AppError::msg("每个组词是 2~4 个字的常用词"));
        }
        if !w.contains(ch) {
            return Err(AppError::msg(format!("每个组词里都要有「{ch}」")));
        }
    }
    // 例句
    if c.sentences.is_empty() {
        return Err(AppError::msg("至少要有一句例句"));
    }
    if c.sentences.len() > 2 {
        return Err(AppError::msg("例句最多两句"));
    }
    for s in &c.sentences {
        if s.contains('\n') {
            return Err(AppError::msg("例句里不能换行"));
        }
        if char_len(s) > 40 {
            return Err(AppError::msg("例句太长了，请压到 40 个字以内"));
        }
        if !s.contains(ch) {
            return Err(AppError::msg(format!("每句例句里都要包含「{ch}」")));
        }
        if let Some(t) = find_term(s) {
            return Err(AppError::msg(format!("例句里用了课堂术语「{t}」，请换成孩子听得懂的说法")));
        }
    }
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_good_payload() {
        let raw = r#"{"meaning":"碧就是青绿色的玉石","mnemonic":"王爷家的白玉石，青绿青绿真好看","words":["碧绿","碧玉"],"sentences":["湖水碧绿得像一块玉石。"]}"#;
        let c = validate(raw, "碧").unwrap();
        assert_eq!(c.meaning, "碧就是青绿色的玉石");
        assert_eq!(c.words.len(), 2);
        assert_eq!(c.sentences.len(), 1);
    }

    #[test]
    fn strips_code_fence_and_parses() {
        let raw = "```json\n{\"meaning\":\"字义\",\"mnemonic\":\"口诀\",\"words\":[\"碧绿\",\"碧玉\"],\"sentences\":[\"湖水碧绿。\"]}\n```";
        assert!(validate(raw, "碧").is_ok());
    }

    #[test]
    fn rejects_broken_json() {
        assert!(validate("我不是 JSON", "碧").unwrap_err().to_string().contains("JSON"));
    }

    #[test]
    fn rejects_empty_or_long_meaning() {
        let base = |m: &str| {
            format!(r#"{{"meaning":"{m}","mnemonic":"口诀","words":["碧绿"],"sentences":["湖水碧绿。"]}}"#)
        };
        assert!(validate(&base(""), "碧").is_err());
        let long = "长".repeat(61);
        assert!(validate(&base(&long), "碧").is_err());
    }

    #[test]
    fn mnemonic_length_boundary_is_40() {
        let base = |m: &str| {
            format!(r#"{{"meaning":"字义","mnemonic":"{m}","words":["碧绿","碧玉"],"sentences":["湖水碧绿。"]}}"#)
        };
        // 40 字整（含标点）：应通过
        let ok = "碧".repeat(39) + "绿";
        assert_eq!(char_len(&ok), 40);
        assert!(validate(&base(&ok), "碧").is_ok(), "40 字口诀应通过");
        // 41 字：应报错
        let long = ok + "碧";
        assert_eq!(char_len(&long), 41);
        assert!(validate(&base(&long), "碧").is_err(), "41 字口诀应报错");
    }

    #[test]
    fn rejects_terms_in_mnemonic_and_sentence() {
        let raw = r#"{"meaning":"字义","mnemonic":"碧的部首是石","words":["碧绿"],"sentences":["湖水碧绿。"]}"#;
        assert!(validate(raw, "碧").unwrap_err().to_string().contains("术语"));
        let raw2 = r#"{"meaning":"字义","mnemonic":"口诀","words":["碧绿","碧玉"],"sentences":["碧绿的笔顺很长。"]}"#;
        assert!(validate(raw2, "碧").unwrap_err().to_string().contains("术语"));
    }

    #[test]
    fn rejects_sentence_missing_char() {
        let raw = r#"{"meaning":"字义","mnemonic":"口诀","words":["碧绿","碧玉"],"sentences":["今天天气很好。"]}"#;
        assert!(validate(raw, "碧").unwrap_err().to_string().contains("碧"));
    }

    #[test]
    fn rejects_bad_words() {
        let base = |w: &str| {
            format!(r#"{{"meaning":"字义","mnemonic":"口诀","words":{w},"sentences":["湖水碧绿。"]}}"#)
        };
        assert!(validate(&base(r#"["绿"]"#), "碧").is_err(), "组词必须含该字");
        assert!(validate(&base(r#"["碧绿"]"#), "碧").is_err(), "至少 2 个组词");
        assert!(
            validate(&base(r#"["碧绿","碧玉","碧空","碧海","碧波"]"#), "碧").is_err(),
            "最多 4 个组词"
        );
    }

    #[test]
    fn rejects_newlines_and_bad_counts() {
        let raw = "{\"meaning\":\"字义\",\"mnemonic\":\"口\\n诀\",\"words\":[\"碧绿\",\"碧玉\"],\"sentences\":[\"湖水碧绿。\"]}";
        assert!(validate(raw, "碧").is_err());
        let raw2 = r#"{"meaning":"字义","mnemonic":"口诀","words":["碧绿","碧玉"],"sentences":["湖水碧绿。","碧空如洗。","太多了。"]}"#;
        assert!(validate(raw2, "碧").is_err());
    }

    #[test]
    fn build_messages_carries_char() {
        let msgs = build_messages(&CharContext { char: "碧".into() });
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "system");
        assert!(msgs[0].content.contains("二年级"));
        assert!(msgs[1].content.contains("碧"));
    }
}
