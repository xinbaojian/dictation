//! 讲解卡领域逻辑：prompt 构造 + LLM 输出校验（纯函数，不依赖 Tauri / DB）
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfusableHint {
    pub char: String,
    pub hint: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardContent {
    #[serde(default)]
    pub decomposition: Vec<String>,
    pub mnemonic: String,
    #[serde(default)]
    pub confusables: Vec<ConfusableHint>,
    pub sentences: Vec<String>,
}

/// 一次讲解卡生成的上下文（repo 组装，domain 只消费）
pub struct WordContext {
    pub text: String,
    pub wrong_chars: Vec<i64>,
    pub wrong_count: i64,
    /// 整词算错（批改没标具体字）的次数：错字为空时它是唯一的强度信号
    pub whole_word_wrong: i64,
}

/// OpenAI 兼容消息（ai/client.rs 消费）
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

const SYSTEM_PROMPT: &str = "你是给中国小学二年级孩子（7~8 岁）讲语文的老师，孩子刚在默写里写错了词语，你要帮他记住怎么写。请严格按以下要求输出：
1. mnemonic：一句能念出声的记忆口诀，不超过 40 个字，朗朗上口，比喻只用孩子熟悉的事物（动物、学校、家里、玩具、食物）。
2. decomposition：把词语里难写的字拆成部件，每个部件一个汉字，给 2~4 个；确实没法拆就给空数组。
3. confusables：0~3 个和它长得像、孩子容易写混的字，每个给一句「怎么分清它们」的小提示；不要用课堂术语。
4. sentences：1~2 个例句，每句不超过 40 个字，必须完整包含这个词语本身，写二年级孩子的生活场景，用词不超纲。
5. 口诀和例句里不许出现这些词：形近字、音近字、部首、音序、笔顺、笔画、偏旁。所有内容都不要换行。
6. 只输出一个 JSON 对象，不要输出 JSON 以外的任何文字，格式示例：
{\"decomposition\":[\"王\",\"白\",\"石\"],\"mnemonic\":\"老王爷坐在白石头上钓鱼\",\"confusables\":[{\"char\":\"壁\",\"hint\":\"下面是土，和墙有关\"}],\"sentences\":[\"湖水碧绿得像一块玉石。\"]}";

/// 课堂术语黑名单：口诀和例句是孩子直接念的，不许出现；辨析 hint（给家长）放宽
const TERMS: [&str; 7] = ["形近字", "音近字", "部首", "音序", "笔顺", "笔画", "偏旁"];

pub fn build_messages(ctx: &WordContext) -> Vec<ChatMessage> {
    let chars: Vec<char> = ctx.text.chars().collect();
    let wrong = if ctx.wrong_chars.is_empty() {
        if ctx.whole_word_wrong > 0 {
            format!("整词写错了 {} 次，没有标具体字", ctx.whole_word_wrong)
        } else {
            "整词写错，没有标具体字".to_string()
        }
    } else {
        let names = ctx
            .wrong_chars
            .iter()
            .filter_map(|i| chars.get(*i as usize).map(|c| c.to_string()))
            .collect::<Vec<_>>()
            .join("、");
        format!("「{names}」")
    };
    let repeat = if ctx.wrong_count >= 3 {
        "\n这个词已经反复错了，请把口诀编得特别顺口好记。"
    } else {
        ""
    };
    vec![
        ChatMessage { role: "system".into(), content: SYSTEM_PROMPT.into() },
        ChatMessage {
            role: "user".into(),
            content: format!(
                "词语：{}\n孩子写错的字：{}\n这个词语已经错了 {} 次。{}",
                ctx.text, wrong, ctx.wrong_count, repeat
            ),
        },
    ]
}

/// LLM 常爱把 JSON 包进 ``` 围栏，先剥掉
pub fn strip_code_fence(raw: &str) -> &str {
    let t = raw.trim();
    let Some(after) = t.strip_prefix("```") else { return t };
    let body = match after.find('\n') {
        Some(i) => &after[i + 1..],
        None => "",
    };
    let body = body.trim();
    match body.strip_suffix("```") {
        Some(b) => b.trim(),
        None => body,
    }
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

fn is_single_char(s: &str) -> bool {
    char_len(s) == 1
}

fn find_term(text: &str) -> Option<&'static str> {
    TERMS.iter().find(|t| text.contains(*t)).copied()
}

/// LLM 输出的唯一入库关卡：解析 → 规整 → 校验，返回规整后的内容。
/// 错误文案要能同时喂回给 LLM 重试、直接展示给家长。
pub fn validate(raw: &str, word: &str) -> AppResult<CardContent> {
    let clean = strip_code_fence(raw);
    let mut c: CardContent = serde_json::from_str(clean)
        .map_err(|_| AppError::msg("AI 输出不是合法的 JSON（缺字段或格式不对）"))?;
    // 规整：去首尾空白，过滤空项
    c.mnemonic = c.mnemonic.trim().to_string();
    c.decomposition = c
        .decomposition
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    c.confusables = c
        .confusables
        .into_iter()
        .map(|f| ConfusableHint { char: f.char.trim().to_string(), hint: f.hint.trim().to_string() })
        .filter(|f| !f.char.is_empty() && !f.hint.is_empty())
        .collect();
    c.sentences = c
        .sentences
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // 口诀
    if c.mnemonic.is_empty() {
        return Err(AppError::msg("口诀不能为空"));
    }
    if c.mnemonic.contains('\n') {
        return Err(AppError::msg("口诀里不能换行"));
    }
    if char_len(&c.mnemonic) > 60 {
        return Err(AppError::msg("口诀太长了，请压到 40 个字以内"));
    }
    if let Some(t) = find_term(&c.mnemonic) {
        return Err(AppError::msg(format!(
            "口诀里用了课堂术语「{t}」，请换成孩子听得懂的说法"
        )));
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
        if !s.contains(word) {
            return Err(AppError::msg(format!("每句例句里都要包含词语「{word}」")));
        }
        if let Some(t) = find_term(s) {
            return Err(AppError::msg(format!(
                "例句里用了课堂术语「{t}」，请换成孩子听得懂的说法"
            )));
        }
    }
    // 部件拆解
    if c.decomposition.len() > 4 {
        return Err(AppError::msg("部件最多拆 4 个"));
    }
    if c.decomposition.iter().any(|s| !is_single_char(s)) {
        return Err(AppError::msg("每个部件必须正好是一个汉字"));
    }
    // 易混字（提示给家长，术语放宽，只查形状与长度）
    if c.confusables.len() > 3 {
        return Err(AppError::msg("易混字最多 3 个"));
    }
    for f in &c.confusables {
        if !is_single_char(&f.char) {
            return Err(AppError::msg("易混字必须正好是一个汉字"));
        }
        if char_len(&f.hint) > 40 {
            return Err(AppError::msg("易混字的提示太长了"));
        }
    }
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> WordContext {
        WordContext { text: "碧绿".into(), wrong_chars: vec![0], wrong_count: 3, whole_word_wrong: 0 }
    }

    const GOOD: &str = r#"{"decomposition":["王","白","石"],"mnemonic":"老王爷坐在白石头上钓鱼","confusables":[{"char":"壁","hint":"下面是土，和墙有关"}],"sentences":["湖水碧绿得像一块玉石。"]}"#;

    #[test]
    fn validates_good_payload() {
        let c = validate(GOOD, "碧绿").unwrap();
        assert_eq!(c.mnemonic, "老王爷坐在白石头上钓鱼");
        assert_eq!(c.decomposition.len(), 3);
        assert_eq!(c.sentences.len(), 1);
    }

    #[test]
    fn strips_code_fence() {
        assert_eq!(
            strip_code_fence("```json\n{ \"a\": 1 }\n```"),
            "{ \"a\": 1 }"
        );
        assert_eq!(strip_code_fence("```\n{ \"a\": 1 }\n```"), "{ \"a\": 1 }");
        assert_eq!(strip_code_fence("  { \"a\": 1 } "), "{ \"a\": 1 }");
    }

    #[test]
    fn rejects_broken_json() {
        let err = validate("我不是 JSON", "碧绿").unwrap_err();
        assert!(err.to_string().contains("JSON"));
    }

    #[test]
    fn rejects_sentence_missing_word() {
        let raw = r#"{"mnemonic":"口诀","sentences":["今天天气很好。"]}"#;
        let err = validate(raw, "碧绿").unwrap_err();
        assert!(err.to_string().contains("碧绿"));
    }

    #[test]
    fn rejects_terms_in_mnemonic_and_sentence() {
        let raw = r#"{"mnemonic":"碧是王白石的形近字口诀","sentences":["碧绿的湖水。"]}"#;
        assert!(validate(raw, "碧绿").unwrap_err().to_string().contains("术语"));
        let raw2 = r#"{"mnemonic":"好口诀","sentences":["碧绿的部首很有趣。"]}"#;
        assert!(validate(raw2, "碧绿").unwrap_err().to_string().contains("术语"));
    }

    #[test]
    fn allows_terms_in_confusable_hint() {
        // 辨析区标「给家长」，术语放宽
        let raw = r#"{"mnemonic":"好口诀","confusables":[{"char":"壁","hint":"部首是土"}],"sentences":["湖水碧绿。"]}"#;
        assert!(validate(raw, "碧绿").is_ok());
    }

    #[test]
    fn rejects_too_long_mnemonic_and_sentences() {
        let long = "好".repeat(61);
        let raw = format!(r#"{{"mnemonic":"{long}","sentences":["湖水碧绿。"]}}"#);
        assert!(validate(&raw, "碧绿").unwrap_err().to_string().contains("口诀"));
        let long_sentence = "美".repeat(41);
        let raw2 = format!(
            r#"{{"mnemonic":"口诀","sentences":["碧绿的{long_sentence}。"]}}"#
        );
        assert!(validate(&raw2, "碧绿").unwrap_err().to_string().contains("例句"));
    }

    #[test]
    fn rejects_newlines_and_bad_shapes() {
        let raw = "{\"mnemonic\":\"口\\n诀\",\"sentences\":[\"湖水碧绿。\"]}";
        assert!(validate(raw, "碧绿").is_err());
        let raw2 = r#"{"mnemonic":"口诀","sentences":[]}"#;
        assert!(validate(raw2, "碧绿").is_err());
        let raw3 = r#"{"mnemonic":"口诀","sentences":["湖水碧绿。","碧绿的山。","太多例句了。"]}"#;
        assert!(validate(raw3, "碧绿").is_err());
        let raw4 = r#"{"mnemonic":"口诀","confusables":[{"char":"壁墙","hint":"x"}],"sentences":["湖水碧绿。"]}"#;
        assert!(validate(raw4, "碧绿").is_err());
        let raw5 = r#"{"mnemonic":"口诀","decomposition":["王白","石"],"sentences":["湖水碧绿。"]}"#;
        assert!(validate(raw5, "碧绿").is_err());
    }

    #[test]
    fn build_messages_carries_context() {
        let msgs = build_messages(&ctx());
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "system");
        assert!(msgs[0].content.contains("二年级"));
        assert!(msgs[1].content.contains("碧绿"));
        assert!(msgs[1].content.contains("「碧」"), "错字下标要翻译成字");
        assert!(msgs[1].content.contains("反复错"), "错 3 次要提示编得特别顺口");
    }

    #[test]
    fn build_messages_without_marks_says_whole_word() {
        let c = WordContext { text: "春天".into(), wrong_chars: vec![], wrong_count: 1, whole_word_wrong: 0 };
        let msgs = build_messages(&c);
        assert!(msgs[1].content.contains("整词写错"));
    }

    #[test]
    fn build_messages_whole_word_count_feeds_prompt() {
        let c = WordContext { text: "春天".into(), wrong_chars: vec![], wrong_count: 4, whole_word_wrong: 4 };
        let msgs = build_messages(&c);
        assert!(
            msgs[1].content.contains("整词写错了 4 次"),
            "整词错次数要进入上下文，让模型知道强度"
        );
    }
}
