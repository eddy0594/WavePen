//! Accuracy helpers shared by speech-to-text providers: vocabulary prompts,
//! silence detection, and filtering of well-known Whisper hallucinations.

use crate::chinese_script;

/// OpenAI documents a 224-token prompt window for Whisper; stay well below it.
const MAX_PROMPT_CHARS: usize = 400;

/// Providers whose transcription endpoint accepts an OpenAI-style `prompt` field.
pub fn provider_supports_prompt(provider: &str) -> bool {
    matches!(
        provider,
        "openai-whisper" | "groq-whisper" | super::config::CUSTOM_WHISPER_PROVIDER
    )
}

/// Label that introduces the vocabulary list; also used to spot prompt echoes.
fn vocabulary_label(chinese_script: &str) -> (&'static str, &'static str, &'static str) {
    match chinese_script::normalize(chinese_script) {
        chinese_script::TRADITIONAL => ("詞彙：", "、", "。"),
        chinese_script::SIMPLIFIED => ("词汇：", "、", "。"),
        _ => ("Vocabulary: ", ", ", "."),
    }
}

/// Builds a short vocabulary prompt from the user's dictionary so custom terms
/// are recognized. Returns `None` when the dictionary is empty: descriptive
/// sentences are avoided because models echo them back on silent audio.
pub fn build_transcription_prompt(dictionary: &[String], chinese_script: &str) -> Option<String> {
    let (label, separator, end) = vocabulary_label(chinese_script);
    let mut terms = String::new();
    for word in dictionary {
        let word = word.trim();
        if word.is_empty() || word.contains(['\n', '\r']) {
            continue;
        }
        let addition = if terms.is_empty() {
            word.to_string()
        } else {
            format!("{separator}{word}")
        };
        if label.chars().count() + terms.chars().count() + addition.chars().count() + 1
            > MAX_PROMPT_CHARS
        {
            break;
        }
        terms.push_str(&addition);
    }
    (!terms.is_empty()).then(|| format!("{label}{terms}{end}"))
}

fn normalized(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// True when the transcript is the prompt (or its vocabulary list) repeated back.
pub fn is_prompt_echo(text: &str, prompt: &str) -> bool {
    let text = normalized(text);
    if text.is_empty() {
        return false;
    }
    let labels = ["詞彙", "词汇", "vocabulary"];
    let terms = labels
        .iter()
        .fold(normalized(prompt), |acc, label| acc.replacen(label, "", 1));
    labels.iter().any(|label| text.contains(label)) || (text.chars().count() >= 4 && text == terms)
}

/// Peak amplitude below which a 16-bit PCM recording is treated as silence
/// (about -41 dBFS). Whisper invents text such as "ご視聴ありがとうございました"
/// when it receives silence, so silent audio is never sent.
const SILENCE_PEAK_THRESHOLD: i32 = 300;
/// Recordings shorter than this are accidental taps.
const MIN_SPEECH_SAMPLES: usize = 16_000 * 3 / 10;

/// Returns true when little-endian 16-bit mono PCM contains no usable speech.
pub fn pcm16_is_silent(pcm: &[u8]) -> bool {
    if pcm.len() / 2 < MIN_SPEECH_SAMPLES {
        return true;
    }
    !pcm.chunks_exact(2).any(|sample| {
        i32::from(i16::from_le_bytes([sample[0], sample[1]])).abs() > SILENCE_PEAK_THRESHOLD
    })
}

/// Phrases Whisper-family models emit for silence or noise (subtitle credits
/// learned from training data). Compared after stripping punctuation and spaces.
const KNOWN_HALLUCINATIONS: &[&str] = &[
    "ご視聴ありがとうございました",
    "ご視聴ありがとうございます",
    "チャンネル登録よろしくお願いします",
    "字幕由amaraorg社群提供",
    "字幕由amaraorg社区提供",
    "請不吝點贊訂閱轉發打賞支持明鏡與點點欄目",
    "请不吝点赞订阅转发打赏支持明镜与点点栏目",
    "謝謝觀看",
    "谢谢观看",
    "感謝觀看",
    "感谢观看",
    "謝謝收看",
    "谢谢收看",
    "thanksforwatching",
    "thankyouforwatching",
    "pleasesubscribe",
    "subtitlesbytheamaraorgcommunity",
    "字幕志愿者",
    "中文字幕志愿者",
    // Prompt sentence sent by WavePen 1.0 pre-release builds.
    "以下是台灣繁體中文與英文夾雜的口述內容",
    "以下是台湾繁体中文与英文夹杂的口述内容",
];

/// Returns true when the whole transcript is a known silence hallucination.
pub fn is_known_hallucination(text: &str) -> bool {
    let normalized = normalized(text);
    if normalized.is_empty() {
        return false;
    }
    KNOWN_HALLUCINATIONS
        .iter()
        .any(|phrase| normalized == *phrase)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(samples: &[i16]) -> Vec<u8> {
        samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    #[test]
    fn prompt_lists_dictionary_terms_in_the_reading_script() {
        let words = [
            "WavePen".to_string(),
            "Typeless".to_string(),
            "潤飾".to_string(),
        ];
        assert_eq!(
            build_transcription_prompt(&words, "traditional").as_deref(),
            Some("詞彙：WavePen、Typeless、潤飾。")
        );
        assert_eq!(
            build_transcription_prompt(&["Kubernetes".to_string()], "preserve").as_deref(),
            Some("Vocabulary: Kubernetes.")
        );
    }

    #[test]
    fn no_prompt_without_dictionary_terms() {
        assert_eq!(build_transcription_prompt(&[], "traditional"), None);
        assert_eq!(
            build_transcription_prompt(&[" ".to_string()], "preserve"),
            None
        );
    }

    #[test]
    fn prompt_is_capped() {
        let words: Vec<String> = (0..500).map(|i| format!("term{i}")).collect();
        let prompt = build_transcription_prompt(&words, "traditional").unwrap();
        assert!(prompt.chars().count() <= MAX_PROMPT_CHARS);
        assert!(prompt.ends_with('。'));
    }

    #[test]
    fn prompt_echoes_are_detected_but_real_speech_with_terms_is_kept() {
        let prompt = "詞彙：WavePen、Typeless、潤飾。";
        assert!(is_prompt_echo("詞彙：WavePen、Typeless、潤飾。", prompt));
        assert!(is_prompt_echo("WavePen Typeless 潤飾", prompt));
        assert!(!is_prompt_echo("我用 WavePen 取代 Typeless", prompt));
        assert!(!is_prompt_echo("潤飾", prompt));
    }

    #[test]
    fn legacy_prompt_sentence_is_treated_as_hallucination() {
        assert!(is_known_hallucination(
            "以下是台灣繁體中文與英文夾雜的口述內容。"
        ));
    }

    #[test]
    fn silence_and_accidental_taps_are_detected() {
        assert!(pcm16_is_silent(&pcm(&vec![120; 16_000])));
        assert!(pcm16_is_silent(&pcm(&vec![8_000; 1_000])));
        let mut speech = vec![50; 16_000];
        speech[8_000] = 4_000;
        assert!(!pcm16_is_silent(&pcm(&speech)));
    }

    #[test]
    fn known_hallucinations_are_filtered_but_real_speech_is_kept() {
        assert!(is_known_hallucination("ご視聴ありがとうございました"));
        assert!(is_known_hallucination("字幕由 Amara.org 社群提供"));
        assert!(is_known_hallucination("Thanks for watching!"));
        assert!(!is_known_hallucination("謝謝觀看這份報告的同事"));
        assert!(!is_known_hallucination(""));
    }

    #[test]
    fn only_openai_compatible_endpoints_receive_prompts() {
        assert!(provider_supports_prompt("openai-whisper"));
        assert!(provider_supports_prompt("groq-whisper"));
        assert!(!provider_supports_prompt("glm-asr"));
        assert!(!provider_supports_prompt("deepgram"));
    }
}
