use serde::Serialize;

/// 粘贴文本切分结果
#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SplitResult {
    pub words: Vec<String>,
    /// 因重复被丢弃的条数
    pub duplicates: usize,
    /// 因不含文字（如纯序号、符号）被丢弃的条数
    pub dropped: usize,
}

const SEPARATORS: &[char] = &[
    '\n', '\r', '\t', ' ', '\u{3000}', '，', ',', '、', ';', '；', '/', '|',
];

/// 把一个词条文本归一化：去掉控制字符与首尾空白
pub fn normalize_word(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_string()
}

/// 切分整段粘贴文本：支持换行/中英文逗号/顿号/分号/斜杠/空白，自动去重与过滤纯序号
pub fn split_words(input: &str) -> SplitResult {
    let mut words: Vec<String> = Vec::new();
    let mut duplicates = 0;
    let mut dropped = 0;

    for raw in input.split(|c: char| SEPARATORS.contains(&c)) {
        let cleaned = normalize_word(&strip_index_prefix(&normalize_word(raw)));
        if cleaned.is_empty() {
            continue;
        }
        if !has_text_char(&cleaned) {
            dropped += 1;
            continue;
        }
        if words.iter().any(|w| w == &cleaned) {
            duplicates += 1;
        } else {
            words.push(cleaned);
        }
    }

    SplitResult { words, duplicates, dropped }
}

/// 去掉行首的题目编号，如 "1." "2、" "3．" "4)" —— 顿号已由分隔符处理，这里只处理点/括号
fn strip_index_prefix(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i > 0 && i < chars.len() && matches!(chars[i], '.' | '．' | ')' | '）' | ':' | '：') {
        return chars[i + 1..].iter().collect::<String>().trim().to_string();
    }
    s.to_string()
}

/// 是否含有可用于默写的文字（汉字或拉丁字母）
fn has_text_char(s: &str) -> bool {
    s.chars().any(|c| {
        c.is_ascii_alphabetic() || ('\u{4e00}'..='\u{9fff}').contains(&c) || ('\u{3400}'..='\u{4dbf}').contains(&c)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(input: &str) -> Vec<String> {
        split_words(input).words
    }

    #[test]
    fn splits_by_newline_and_comma() {
        assert_eq!(
            words("春天 夏天\n秋天,冬天"),
            vec!["春天", "夏天", "秋天", "冬天"]
        );
    }

    #[test]
    fn splits_chinese_punctuation() {
        assert_eq!(words("苹果，香蕉、西瓜；葡萄"), vec!["苹果", "香蕉", "西瓜", "葡萄"]);
    }

    #[test]
    fn dedupes_and_reports() {
        let r = split_words("苹果，苹果，香蕉");
        assert_eq!(r.words, vec!["苹果", "香蕉"]);
        assert_eq!(r.duplicates, 1);
    }

    #[test]
    fn strips_index_prefix() {
        assert_eq!(words("1. 春天\n2、夏天\n3．秋天"), vec!["春天", "夏天", "秋天"]);
    }

    #[test]
    fn drops_pure_symbols() {
        let r = split_words("语文 123 ---");
        assert_eq!(r.words, vec!["语文"]);
        assert_eq!(r.dropped, 2);
    }

    #[test]
    fn keeps_latin_words() {
        assert_eq!(words("ABC 猫"), vec!["ABC", "猫"]);
    }

    #[test]
    fn empty_input() {
        assert_eq!(split_words(" \n　\t "), SplitResult { words: vec![], duplicates: 0, dropped: 0 });
    }

    #[test]
    fn full_width_space_trimmed() {
        assert_eq!(normalize_word("　春天　"), "春天");
    }
}
