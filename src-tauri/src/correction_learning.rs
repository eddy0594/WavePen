//! Learns dictionary corrections from edits the user makes right after dictation.
//!
//! After WavePen types text into another app, the focused text field is read
//! through the macOS Accessibility API for a short while. When the user replaces
//! a word inside the text that was just inserted (for example 論識 → 潤飾), the
//! replacement becomes a correction rule plus a dictionary term so the next
//! transcription and polish pass get it right. Field contents are only compared
//! in memory and are never stored or sent anywhere.

use serde::Serialize;

/// Longest inserted text that is diffed (keeps the O(n·m) diff cheap).
const MAX_DIFF_CHARS: usize = 600;
/// Longest phrase learned as a single correction.
const MAX_PHRASE_CHARS: usize = 16;
/// More separate edits than this looks like a rewrite, not a correction.
const MAX_HUNKS: usize = 3;
/// Characters of surrounding field text used to find the insertion again.
const ANCHOR_CHARS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearnedCorrection {
    pub pattern: String,
    pub replacement: String,
}

/// Where the dictated text sits inside the focused field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsertionAnchor {
    pub before: String,
    pub inserted: String,
    pub after: String,
    /// Character offset of the insertion, used to pick the right `before` match.
    pub offset: usize,
}

fn normalize_newlines(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

/// Locates the most recent occurrence of `inserted` in the field value.
pub fn anchor_from_snapshot(field_value: &str, inserted: &str) -> Option<InsertionAnchor> {
    let field: Vec<char> = normalize_newlines(field_value).chars().collect();
    let inserted: Vec<char> = normalize_newlines(inserted).trim().chars().collect();
    if inserted.is_empty() || inserted.len() > field.len() || inserted.len() > MAX_DIFF_CHARS {
        return None;
    }
    let start = (0..=field.len() - inserted.len())
        .rev()
        .find(|&start| field[start..start + inserted.len()] == inserted[..])?;
    let end = start + inserted.len();
    Some(InsertionAnchor {
        before: field[start.saturating_sub(ANCHOR_CHARS)..start]
            .iter()
            .collect(),
        inserted: inserted.iter().collect(),
        after: field[end..(end + ANCHOR_CHARS).min(field.len())]
            .iter()
            .collect(),
        offset: start,
    })
}

/// Returns the current text between the anchors, i.e. the dictated text as the
/// user has edited it. `None` when the surroundings changed too much to tell.
pub fn edited_segment(anchor: &InsertionAnchor, field_value: &str) -> Option<String> {
    let field: Vec<char> = normalize_newlines(field_value).chars().collect();
    let before: Vec<char> = anchor.before.chars().collect();
    let after: Vec<char> = anchor.after.chars().collect();

    let start = if before.is_empty() {
        0
    } else {
        let expected = anchor.offset.saturating_sub(before.len());
        (0..=field.len().checked_sub(before.len())?)
            .filter(|&index| field[index..index + before.len()] == before[..])
            .min_by_key(|&index| index.abs_diff(expected))?
            + before.len()
    };
    let end = if after.is_empty() {
        field.len()
    } else {
        (start..=field.len().checked_sub(after.len())?)
            .find(|&index| field[index..index + after.len()] == after[..])?
    };
    let segment: String = field[start..end].iter().collect();
    Some(segment.trim().to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Hunk {
    old_start: usize,
    old_end: usize,
    new_start: usize,
    new_end: usize,
}

/// Character-level diff hunks between `old` and `new` using an LCS table.
fn diff_hunks(old: &[char], new: &[char]) -> Vec<Hunk> {
    let (n, m) = (old.len(), new.len());
    let mut lcs = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if old[i] == new[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    let mut hunks = Vec::new();
    let (mut i, mut j) = (0, 0);
    let mut open: Option<(usize, usize)> = None;
    while i < n || j < m {
        if i < n && j < m && old[i] == new[j] {
            if let Some((old_start, new_start)) = open.take() {
                hunks.push(Hunk {
                    old_start,
                    old_end: i,
                    new_start,
                    new_end: j,
                });
            }
            i += 1;
            j += 1;
            continue;
        }
        open.get_or_insert((i, j));
        if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
            j += 1;
        } else {
            i += 1;
        }
    }
    if let Some((old_start, new_start)) = open {
        hunks.push(Hunk {
            old_start,
            old_end: n,
            new_start,
            new_end: m,
        });
    }
    hunks
}

fn is_cjk(c: char) -> bool {
    matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}')
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// Grows a hunk to whole English words, and to at least two characters for
/// Chinese so a single-character rule (語 → 一) never gets learned.
fn expand_hunk(mut hunk: Hunk, old: &[char], new: &[char]) -> Hunk {
    let touches_word = |h: &Hunk| {
        old[h.old_start..h.old_end]
            .iter()
            .chain(&new[h.new_start..h.new_end])
            .any(|c| is_word_char(*c))
    };
    if touches_word(&hunk) {
        while hunk.old_start > 0 && hunk.new_start > 0 && is_word_char(old[hunk.old_start - 1]) {
            hunk.old_start -= 1;
            hunk.new_start -= 1;
        }
        while hunk.old_end < old.len()
            && hunk.new_end < new.len()
            && is_word_char(old[hunk.old_end])
        {
            hunk.old_end += 1;
            hunk.new_end += 1;
        }
    }

    let old_is_short_cjk = hunk.old_end - hunk.old_start < 2
        && old[hunk.old_start..hunk.old_end].iter().all(|c| is_cjk(*c));
    if old_is_short_cjk {
        if hunk.old_end < old.len() && hunk.new_end < new.len() && is_cjk(old[hunk.old_end]) {
            hunk.old_end += 1;
            hunk.new_end += 1;
        } else if hunk.old_start > 0 && hunk.new_start > 0 && is_cjk(old[hunk.old_start - 1]) {
            hunk.old_start -= 1;
            hunk.new_start -= 1;
        }
    }
    hunk
}

fn meaningful(value: &str) -> String {
    value.chars().filter(|c| c.is_alphanumeric()).collect()
}

/// Word or phrase replacements that turn `original` into `edited`.
/// Pure insertions, deletions, punctuation fixes, and rewrites are ignored.
pub fn extract_corrections(original: &str, edited: &str) -> Vec<LearnedCorrection> {
    let old: Vec<char> = original.chars().collect();
    let new: Vec<char> = edited.chars().collect();
    if old == new || old.is_empty() || old.len() > MAX_DIFF_CHARS || new.len() > MAX_DIFF_CHARS {
        return Vec::new();
    }

    let hunks = diff_hunks(&old, &new);
    let changed: usize = hunks.iter().map(|h| h.old_end - h.old_start).sum();
    if hunks.len() > MAX_HUNKS || changed * 2 > old.len().max(4) {
        return Vec::new();
    }

    let mut learned: Vec<LearnedCorrection> = Vec::new();
    let mut previous_end = 0;
    for hunk in hunks {
        if hunk.old_start == hunk.old_end || hunk.new_start == hunk.new_end {
            continue;
        }
        let hunk = expand_hunk(hunk, &old, &new);
        if hunk.old_start < previous_end {
            return Vec::new();
        }
        previous_end = hunk.old_end;

        let pattern: String = old[hunk.old_start..hunk.old_end].iter().collect();
        let replacement: String = new[hunk.new_start..hunk.new_end].iter().collect();
        let (pattern, replacement) = (pattern.trim(), replacement.trim());
        let usable = !pattern.is_empty()
            && !replacement.is_empty()
            && pattern.chars().count() <= MAX_PHRASE_CHARS
            && replacement.chars().count() <= MAX_PHRASE_CHARS
            && !replacement.contains('\n')
            && meaningful(pattern) != meaningful(replacement)
            && !meaningful(replacement).is_empty();
        if usable {
            learned.push(LearnedCorrection {
                pattern: pattern.to_string(),
                replacement: replacement.to_string(),
            });
        }
    }
    learned
}

#[cfg(target_os = "macos")]
pub use platform::watch_inserted_text;

#[cfg(not(target_os = "macos"))]
pub fn watch_inserted_text(_app: tauri::AppHandle, _inserted: String) {}

#[cfg(target_os = "macos")]
mod platform {
    use super::{anchor_from_snapshot, edited_segment, extract_corrections, LearnedCorrection};
    use std::ffi::{c_char, c_void, CStr};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};
    use tauri::{Emitter, Manager};

    type CfTypeRef = *const c_void;
    const UTF8: u32 = 0x0800_0100;
    const AX_SUCCESS: i32 = 0;

    const POLL_INTERVAL: Duration = Duration::from_millis(600);
    const SETTLE_AFTER_EDIT: Duration = Duration::from_secs(3);
    const WATCH_DURATION: Duration = Duration::from_secs(60);

    /// Bumped for every dictation so an older watcher stops.
    static GENERATION: AtomicU64 = AtomicU64::new(0);

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateSystemWide() -> CfTypeRef;
        fn AXUIElementCopyAttributeValue(
            element: CfTypeRef,
            attribute: CfTypeRef,
            value: *mut CfTypeRef,
        ) -> i32;
        fn AXUIElementSetMessagingTimeout(element: CfTypeRef, seconds: f32) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: CfTypeRef);
        fn CFEqual(left: CfTypeRef, right: CfTypeRef) -> u8;
        fn CFGetTypeID(cf: CfTypeRef) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFStringCreateWithBytes(
            allocator: CfTypeRef,
            bytes: *const u8,
            length: isize,
            encoding: u32,
            external: u8,
        ) -> CfTypeRef;
        fn CFStringGetLength(string: CfTypeRef) -> isize;
        fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
        fn CFStringGetCString(
            string: CfTypeRef,
            buffer: *mut c_char,
            size: isize,
            encoding: u32,
        ) -> u8;
    }

    /// Owned Core Foundation reference, released on drop.
    struct Cf(CfTypeRef);

    impl Cf {
        fn new(value: CfTypeRef) -> Option<Self> {
            (!value.is_null()).then_some(Self(value))
        }

        fn string(value: &str) -> Option<Self> {
            Self::new(unsafe {
                CFStringCreateWithBytes(
                    std::ptr::null(),
                    value.as_ptr(),
                    value.len() as isize,
                    UTF8,
                    0,
                )
            })
        }

        fn to_rust_string(&self) -> Option<String> {
            unsafe {
                if CFGetTypeID(self.0) != CFStringGetTypeID() {
                    return None;
                }
                let size = CFStringGetMaximumSizeForEncoding(CFStringGetLength(self.0), UTF8) + 1;
                let mut buffer = vec![0 as c_char; usize::try_from(size).ok()?];
                if CFStringGetCString(self.0, buffer.as_mut_ptr(), size, UTF8) == 0 {
                    return None;
                }
                Some(
                    CStr::from_ptr(buffer.as_ptr())
                        .to_string_lossy()
                        .into_owned(),
                )
            }
        }

        fn same_as(&self, other: &Cf) -> bool {
            unsafe { CFEqual(self.0, other.0) != 0 }
        }
    }

    impl Drop for Cf {
        fn drop(&mut self) {
            unsafe { CFRelease(self.0) };
        }
    }

    fn attribute(element: &Cf, name: &str) -> Option<Cf> {
        let name = Cf::string(name)?;
        let mut value: CfTypeRef = std::ptr::null();
        let status = unsafe { AXUIElementCopyAttributeValue(element.0, name.0, &mut value) };
        if status == AX_SUCCESS {
            Cf::new(value)
        } else {
            None
        }
    }

    fn focused_element() -> Option<Cf> {
        let system = Cf::new(unsafe { AXUIElementCreateSystemWide() })?;
        unsafe { AXUIElementSetMessagingTimeout(system.0, 0.5) };
        let element = attribute(&system, "AXFocusedUIElement")?;
        unsafe { AXUIElementSetMessagingTimeout(element.0, 0.5) };
        Some(element)
    }

    fn string_attribute(element: &Cf, name: &str) -> Option<String> {
        attribute(element, name)?.to_rust_string()
    }

    fn is_secure_field(element: &Cf) -> bool {
        let role = string_attribute(element, "AXRole").unwrap_or_default();
        let subrole = string_attribute(element, "AXSubrole").unwrap_or_default();
        role == "AXSecureTextField" || subrole == "AXSecureTextField"
    }

    /// Watches the focused field for about a minute after dictation and learns
    /// word replacements the user makes inside the inserted text.
    pub fn watch_inserted_text(app: tauri::AppHandle, inserted: String) {
        let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        let spawned = std::thread::Builder::new()
            .name("wavepen-correction-learning".to_string())
            .spawn(move || watch(app, inserted, generation));
        if let Err(error) = spawned {
            tracing::warn!("Failed to start correction learning: {error}");
        }
    }

    fn watch(app: tauri::AppHandle, inserted: String, generation: u64) {
        // Give the target app a moment to apply the synthetic keystrokes.
        std::thread::sleep(Duration::from_millis(400));
        let Some(element) = focused_element() else {
            return;
        };
        if is_secure_field(&element) {
            return;
        }
        let Some(initial) = string_attribute(&element, "AXValue") else {
            tracing::debug!("Correction learning unavailable: focused field has no readable value");
            return;
        };
        let Some(mut anchor) = anchor_from_snapshot(&initial, &inserted) else {
            tracing::debug!("Correction learning unavailable: inserted text not found in field");
            return;
        };

        let started = Instant::now();
        let mut baseline = initial.clone();
        let mut latest = initial;
        let mut last_change = Instant::now();

        while started.elapsed() < WATCH_DURATION {
            std::thread::sleep(POLL_INTERVAL);
            if GENERATION.load(Ordering::SeqCst) != generation {
                return;
            }
            let still_focused = focused_element().is_some_and(|focused| focused.same_as(&element));
            let Some(current) = string_attribute(&element, "AXValue") else {
                return;
            };
            if current != latest {
                latest = current;
                last_change = Instant::now();
            }

            let settled = last_change.elapsed() >= SETTLE_AFTER_EDIT;
            if latest != baseline && (settled || !still_focused) {
                let Some(segment) = edited_segment(&anchor, &latest) else {
                    return;
                };
                let corrections = extract_corrections(&anchor.inserted, &segment);
                if !corrections.is_empty() {
                    learn(&app, corrections);
                }
                match anchor_from_snapshot(&latest, &segment) {
                    Some(next) => anchor = next,
                    None => return,
                }
                baseline = latest.clone();
            }
            if !still_focused {
                return;
            }
        }
    }

    #[derive(Clone, serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct LearnedPayload {
        pattern: String,
        replacement: String,
        added_word: bool,
    }

    fn learn(app: &tauri::AppHandle, corrections: Vec<LearnedCorrection>) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let store = app.state::<crate::storage::DictionaryStore>();
            for correction in corrections {
                if let Err(error) = store
                    .add_correction(&correction.pattern, &correction.replacement)
                    .await
                {
                    tracing::debug!("Skipped learned correction: {error}");
                    continue;
                }
                let known = store
                    .words()
                    .await
                    .iter()
                    .any(|word| word.eq_ignore_ascii_case(&correction.replacement));
                let added_word = !known && store.add(&correction.replacement, None).await.is_ok();
                tracing::info!(
                    "Learned correction ({} -> {} chars)",
                    correction.pattern.chars().count(),
                    correction.replacement.chars().count()
                );
                let _ = app.emit(
                    "dictionary:learned",
                    LearnedPayload {
                        pattern: correction.pattern,
                        replacement: correction.replacement,
                        added_word,
                    },
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(original: &str, edited: &str) -> Vec<(String, String)> {
        extract_corrections(original, edited)
            .into_iter()
            .map(|c| (c.pattern, c.replacement))
            .collect()
    }

    #[test]
    fn learns_chinese_homophone_replacement() {
        assert_eq!(
            pairs("它會不會影響到論識呢？", "它會不會影響到潤飾呢？"),
            vec![("論識".to_string(), "潤飾".to_string())]
        );
    }

    #[test]
    fn single_chinese_character_fix_is_widened_to_a_phrase() {
        assert_eq!(
            pairs("也按了兩次語律允許", "也按了兩次一律允許"),
            vec![("語律".to_string(), "一律".to_string())]
        );
    }

    #[test]
    fn english_fix_is_widened_to_the_whole_word() {
        assert_eq!(
            pairs("在使用 tabless 的時候", "在使用 Typeless 的時候"),
            vec![("tabless".to_string(), "Typeless".to_string())]
        );
    }

    #[test]
    fn multi_word_name_is_learned_as_one_phrase() {
        assert_eq!(
            pairs("我使用這個 Say Right 的時候", "我使用這個 SayWrite 的時候"),
            vec![("Say Right".to_string(), "SayWrite".to_string())]
        );
    }

    #[test]
    fn ignores_punctuation_insertions_deletions_and_rewrites() {
        assert!(pairs("今天天氣很好", "今天天氣很好。").is_empty());
        assert!(pairs("今天天氣很好我們出去", "今天天氣很好，我們出去").is_empty());
        assert!(pairs("今天天氣很好", "今天很好").is_empty());
        assert!(pairs("今天天氣很好", "明日下雨了嗎").is_empty());
    }

    #[test]
    fn anchors_find_the_edited_dictation_inside_a_larger_field() {
        let before = "Hi Amy,\n這是論識後的內容\nThanks";
        let anchor = anchor_from_snapshot(before, "這是論識後的內容").unwrap();
        let after = "Hi Amy,\n這是潤飾後的內容\nThanks, Bob";
        let segment = edited_segment(&anchor, after).unwrap();
        assert_eq!(segment, "這是潤飾後的內容");
        assert_eq!(
            pairs(&anchor.inserted, &segment),
            vec![("論識".to_string(), "潤飾".to_string())]
        );
    }

    #[test]
    fn anchor_uses_the_latest_occurrence_and_handles_field_edges() {
        let anchor = anchor_from_snapshot("好的 好的", "好的").unwrap();
        assert_eq!(anchor.offset, 3);
        assert_eq!(anchor.after, "");
        assert_eq!(
            edited_segment(&anchor, "好的 好滴").as_deref(),
            Some("好滴")
        );
        assert!(anchor_from_snapshot("abc", "xyz").is_none());
    }
}
