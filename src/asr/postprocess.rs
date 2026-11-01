//! ASR 文本后处理：标点补全与符号规范化 (纯规则，零模型依赖)
//!
//! 对齐官方"符号自动转换"与"智能标点"的可用子集：
//! - 数字间的全角冒号转半角 (12：30 -> 12:30)
//! - 折叠连续重复的标点
//! - 依据中英文占比补全句末标点

/// 句末标点集合 (出现即认为句子已完结，不再追加)
const ENDING_PUNCT: &[char] = &['。', '！', '？', '…', '；', '.', '!', '?', ';', '~', '～'];

/// 需要折叠重复的标点集合
const COLLAPSIBLE: &[char] = &['。', '，', '！', '？', '；', '、', '.', ',', '!', '?', ';'];

/// 判断字符是否为 CJK (用于中英文占比判定)
fn is_cjk(c: char) -> bool {
    ('\u{4E00}'..='\u{9FFF}').contains(&c)
        || ('\u{3400}'..='\u{4DBF}').contains(&c)
        || ('\u{3000}'..='\u{303F}').contains(&c) // CJK 标点也算中文语境
}

/// 数字间的全角冒号转半角 (对齐官方"符号自动转换": 12：00 -> 12:00)
fn normalize_fullwidth_colon(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        if c == '：'
            && i > 0
            && i + 1 < chars.len()
            && chars[i - 1].is_ascii_digit()
            && chars[i + 1].is_ascii_digit()
        {
            out.push(':');
        } else {
            out.push(c);
        }
    }
    out
}

/// 折叠连续重复的标点 ("好的，，好的" -> "好的，好的")
fn collapse_repeated_punct(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_punct: Option<char> = None;
    for c in text.chars() {
        if COLLAPSIBLE.contains(&c) {
            if prev_punct == Some(c) {
                continue; // 与上一个相同的可折叠标点，跳过
            }
            prev_punct = Some(c);
        } else {
            prev_punct = None;
        }
        out.push(c);
    }
    out
}

/// 判断文本是否以任意句末标点结尾
fn ends_with_punct(text: &str) -> bool {
    text.chars().last().map_or(false, |c| ENDING_PUNCT.contains(&c))
}

/// 依据中英文占比选择句末标点 (中文 "。" / 西文 ".")
fn sentence_end(text: &str) -> &'static str {
    let cjk = text.chars().filter(|c| is_cjk(*c)).count();
    let ascii_letters = text.chars().filter(|c| c.is_ascii_alphabetic()).count();
    if cjk >= ascii_letters {
        "。"
    } else {
        "."
    }
}

/// 后处理入口：清理 + 规范化 + (可选) 句末标点补全
pub fn polish(text: &str, auto_punctuation: bool) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let normalized = collapse_repeated_punct(&normalize_fullwidth_colon(trimmed));
    if auto_punctuation && !ends_with_punct(&normalized) {
        let mut out = normalized;
        out.push_str(sentence_end(&out));
        out
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_cjk_period() {
        assert_eq!(polish("你好世界", true), "你好世界。");
    }

    #[test]
    fn appends_ascii_period_for_english() {
        assert_eq!(polish("hello world", true), "hello world.");
    }

    #[test]
    fn keeps_existing_ending_punct() {
        assert_eq!(polish("你好世界！", true), "你好世界！");
        assert_eq!(polish("done?", true), "done?");
    }

    #[test]
    fn normalizes_fullwidth_colon_in_numbers() {
        assert_eq!(polish("会议在12：30开始", true), "会议在12:30开始。");
        // 非数字语境的全角冒号保持原样
        assert_eq!(polish("注意：安全", true), "注意：安全。");
    }

    #[test]
    fn collapses_repeated_punct() {
        assert_eq!(polish("好的，，好的", true), "好的，好的。");
        assert_eq!(polish("等一下。。。", true), "等一下。");
    }

    #[test]
    fn no_punct_when_disabled() {
        assert_eq!(polish("你好世界", false), "你好世界");
    }

    #[test]
    fn empty_and_whitespace() {
        assert_eq!(polish("", true), "");
        assert_eq!(polish("   ", true), "");
    }
}
