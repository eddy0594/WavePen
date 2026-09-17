//! Deterministic Chinese script conversion for transcripts and LLM output.
//!
//! Speech-to-text engines (Whisper in particular) frequently return Simplified
//! Chinese even for Taiwanese speakers, and LLMs tend to follow whatever script
//! the transcript used. Converting with OpenCC/MediaWiki rules is predictable,
//! offline, and does not depend on prompt compliance.

use std::sync::OnceLock;

use ferrous_opencc::{config::BuiltinConfig, OpenCC};

pub const PRESERVE: &str = "preserve";
pub const TRADITIONAL: &str = "traditional";
pub const SIMPLIFIED: &str = "simplified";

/// Returns a supported script value, falling back to `preserve`.
pub fn normalize(script: &str) -> &'static str {
    match script.trim() {
        TRADITIONAL => TRADITIONAL,
        SIMPLIFIED => SIMPLIFIED,
        _ => PRESERVE,
    }
}

/// Converts `text` to the configured script. `traditional` uses Taiwan
/// vocabulary (e.g. 软件 → 軟體), `simplified` uses Mainland vocabulary.
pub fn convert(text: &str, script: &str) -> String {
    let script = normalize(script);
    if script == PRESERVE || !text.chars().any(is_cjk_ideograph) {
        return text.to_string();
    }
    match converter(script) {
        // OpenCC emits the formal 臺; everyday Taiwanese writing uses 台 (台灣, 台北).
        Some(converter) if script == TRADITIONAL => converter.convert(text).replace('臺', "台"),
        Some(converter) => converter.convert(text),
        None => text.to_string(),
    }
}

fn converter(script: &str) -> Option<&'static OpenCC> {
    static TO_TRADITIONAL: OnceLock<Option<OpenCC>> = OnceLock::new();
    static TO_SIMPLIFIED: OnceLock<Option<OpenCC>> = OnceLock::new();

    let (cell, config) = match script {
        TRADITIONAL => (&TO_TRADITIONAL, BuiltinConfig::S2twp),
        SIMPLIFIED => (&TO_SIMPLIFIED, BuiltinConfig::Tw2sp),
        _ => return None,
    };
    cell.get_or_init(|| {
        OpenCC::from_config(config)
            .inspect_err(|error| {
                tracing::warn!("Failed to load OpenCC {script} dictionary: {error}")
            })
            .ok()
    })
    .as_ref()
}

/// Converts streamed LLM chunks without splitting multi-character phrases.
///
/// OpenCC converts phrases such as 文件 → 檔案, so text is only released up to
/// the last punctuation or whitespace boundary; the tail waits for more chunks.
pub struct StreamingConverter {
    script: &'static str,
    pending: String,
}

/// Longest run held back while waiting for a boundary.
const STREAMING_MAX_PENDING_CHARS: usize = 24;

impl StreamingConverter {
    pub fn new(script: &str) -> Self {
        Self {
            script: normalize(script),
            pending: String::new(),
        }
    }

    /// Returns converted text that is safe to emit now, if any.
    pub fn push(&mut self, chunk: &str) -> Option<String> {
        if self.script == PRESERVE {
            return (!chunk.is_empty()).then(|| chunk.to_string());
        }
        self.pending.push_str(chunk);
        let split_at = match self.pending.rfind(is_phrase_boundary) {
            Some(index) => {
                index
                    + self.pending[index..]
                        .chars()
                        .next()
                        .map_or(0, char::len_utf8)
            }
            None if self.pending.chars().count() > STREAMING_MAX_PENDING_CHARS => {
                self.pending.len()
            }
            None => return None,
        };
        let ready: String = self.pending.drain(..split_at).collect();
        Some(convert(&ready, self.script))
    }

    /// Returns any remaining converted text at the end of the stream.
    pub fn finish(&mut self) -> Option<String> {
        if self.pending.is_empty() {
            return None;
        }
        let rest = std::mem::take(&mut self.pending);
        Some(convert(&rest, self.script))
    }
}

fn is_phrase_boundary(c: char) -> bool {
    c.is_whitespace()
        || c.is_ascii_punctuation()
        || "，。！？；：、「」『』（）《》〈〉…—".contains(c)
}

fn is_cjk_ideograph(c: char) -> bool {
    matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traditional_uses_taiwan_vocabulary() {
        assert_eq!(
            convert("这个软件的网络很简单", TRADITIONAL),
            "這個軟體的網路很簡單"
        );
    }

    #[test]
    fn traditional_converts_common_dictation_phrases() {
        assert_eq!(
            convert("请把这个文件保存到桌面，然后打开鼠标设置", TRADITIONAL),
            "請把這個檔案儲存到桌面，然後開啟滑鼠設定"
        );
    }

    #[test]
    fn traditional_prefers_everyday_tai_character() {
        assert_eq!(convert("台湾台北", TRADITIONAL), "台灣台北");
    }

    #[test]
    fn simplified_converts_traditional_text() {
        assert_eq!(
            convert("這個軟體的網路很簡單", SIMPLIFIED),
            "这个软件的网络很简单"
        );
    }

    #[test]
    fn preserve_and_unknown_values_leave_text_unchanged() {
        assert_eq!(convert("这个软件", PRESERVE), "这个软件");
        assert_eq!(convert("这个软件", "bogus"), "这个软件");
    }

    #[test]
    fn english_and_mixed_text_keep_latin_words() {
        assert_eq!(convert("Send the email", TRADITIONAL), "Send the email");
        assert_eq!(
            convert("帮我把 README 发到 GitHub", TRADITIONAL),
            "幫我把 README 發到 GitHub"
        );
    }

    #[test]
    fn streaming_converter_keeps_phrases_split_across_chunks() {
        let mut converter = StreamingConverter::new(TRADITIONAL);
        let mut output = String::new();
        for chunk in ["请把这个文", "件保存到桌面，然后打", "开鼠标设置"] {
            if let Some(ready) = converter.push(chunk) {
                output.push_str(&ready);
            }
        }
        output.push_str(&converter.finish().unwrap_or_default());
        assert_eq!(output, "請把這個檔案儲存到桌面，然後開啟滑鼠設定");
        assert_eq!(
            output,
            convert("请把这个文件保存到桌面，然后打开鼠标设置", TRADITIONAL)
        );
    }

    #[test]
    fn streaming_converter_passes_chunks_through_when_preserving() {
        let mut converter = StreamingConverter::new(PRESERVE);
        assert_eq!(converter.push("这个").as_deref(), Some("这个"));
        assert_eq!(converter.finish(), None);
    }

    #[test]
    fn streaming_converter_flushes_long_runs_without_punctuation() {
        let mut converter = StreamingConverter::new(TRADITIONAL);
        assert!(converter.push("这是一段没有标点").is_none());
        assert!(converter
            .push("而且是一个非常非常非常非常非常长的句子")
            .is_some());
    }

    #[test]
    fn normalize_rejects_unknown_values() {
        assert_eq!(normalize(" traditional "), TRADITIONAL);
        assert_eq!(normalize(""), PRESERVE);
    }
}
