//! Round number extraction from OCR text.
//!
//! Parses round numbers from game screens using strict positional rules:
//! - "XX ラウンド終了": number BEFORE "ラウンド" only
//! - "XX ラウンド" (selection panel): number BEFORE "ラウンド" only
//! - "自動周回中（X/Y）": X from the parenthesized fraction
//!
//! Numbers AFTER "ラウンド終了" (decoration artifacts like `1 い Ⅱ 』`) are ignored.

/// Extract a round number (1-99) from the digits immediately BEFORE "ラウンド".
///
/// Only considers the last 1-2 digit sequence that appears before the
/// "ラウンド" marker. Digits after "ラウンド" are ignored to avoid
/// picking up decoration artifacts from the OCR.
///
/// # Examples
///
/// ```
/// use dna_detector::round_number::parse;
///
/// assert_eq!(parse("22 ラウンド終了 1 い Ⅱ"), Some(22));
/// assert_eq!(parse("1 ラウンド終了"), Some(1));
/// assert_eq!(parse("03 ラウンド"), Some(3));
/// assert_eq!(parse("ラウンド終了 1 い Ⅱ 』"), None); // no number before ラウンド
/// assert_eq!(parse("ラウンド"), None);
/// assert_eq!(parse("damage 1234"), None);
/// ```
#[must_use]
pub fn parse(text: &str) -> Option<u32> {
    let normalized: String = text.chars().filter(|c| !c.is_whitespace()).collect();

    // Find "ラウンド" and only look at text before it
    let before = normalized.split("ラウンド").next()?;
    if before.len() == normalized.len() {
        // "ラウンド" not found
        return None;
    }

    // Extract the last 1-2 digit sequence before "ラウンド"
    let mut last_valid: Option<u32> = None;
    let mut digits = String::new();
    for ch in before.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else {
            if let Some(num) = parse_1_2_digits(&digits) {
                last_valid = Some(num);
            }
            digits.clear();
        }
    }
    // Check trailing digits (immediately before "ラウンド")
    if let Some(num) = parse_1_2_digits(&digits) {
        last_valid = Some(num);
    }
    last_valid
}

/// Parse the round number from a "自動周回中（X/Y）" header.
///
/// Extracts X from the pattern `周回中（X/Y）` or `周回中(X/Y)`.
/// Handles OCR artifacts like spaces, colons, and apostrophes.
///
/// # Examples
///
/// ```
/// use dna_detector::round_number::parse_select_header;
///
/// assert_eq!(parse_select_header("自動周回中（13/99）"), Some(13));
/// assert_eq!(parse_select_header("自 動 周 回 中 ( 1 / 5 )"), Some(1));
/// assert_eq!(parse_select_header("自 動 周 回 中 : 98 ′ 9"), Some(98));
/// ```
#[must_use]
pub fn parse_select_header(text: &str) -> Option<u32> {
    let normalized: String = text.chars().filter(|c| !c.is_whitespace()).collect();

    // Find text after "周回中"
    let after = normalized.split("周回中").nth(1)?;

    // Skip opening bracket/colon: （, (, :, etc.
    let after = after.trim_start_matches(['（', '(', ':', '：', '′', '\'']);

    // Extract X from "X/Y" — stop at '/' to avoid picking up Y on misread
    let before_slash = after.split('/').next().unwrap_or(after);

    // Extract first 1-2 digit sequence from X portion only
    let mut digits = String::new();
    for ch in before_slash.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else if !digits.is_empty() {
            break;
        }
    }
    parse_1_2_digits(&digits)
}

/// Check if OCR text indicates a round end screen ("ラウンド終了").
#[must_use]
pub fn is_round_end_text(text: &str) -> bool {
    let normalized: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    normalized.contains("ラウンド終了")
}

/// Check if OCR text indicates a round selection screen ("自動周回中").
#[must_use]
pub fn is_round_select_text(text: &str) -> bool {
    let normalized: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    normalized.contains("自動周回中")
}

/// Parse a 1-2 digit string as a round number (1-99).
fn parse_1_2_digits(digits: &str) -> Option<u32> {
    if !(1..=2).contains(&digits.len()) {
        return None;
    }
    let num: u32 = match digits.parse() {
        Ok(n) => n,
        Err(_) => return None,
    };
    if (1..=99).contains(&num) {
        Some(num)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse ---

    #[test]
    fn round_end_two_digit() {
        assert_eq!(parse("22 ラウンド終了"), Some(22));
        assert_eq!(parse("99 ラウンド終了"), Some(99));
    }

    #[test]
    fn round_end_single_digit() {
        assert_eq!(parse("1 ラウンド終了"), Some(1));
        assert_eq!(parse("6 ラウンド終了"), Some(6));
    }

    #[test]
    fn round_end_leading_zero() {
        assert_eq!(parse("01 ラウンド終了"), Some(1));
        assert_eq!(parse("03 ラウンド終了"), Some(3));
    }

    #[test]
    fn round_end_ignores_trailing_artifacts() {
        // Real OCR: "ラウンド終了 1 い Ⅱ 』" — the 1 is decoration, not the round
        assert_eq!(parse("ラウンド終了 1 い Ⅱ 』"), None);
        assert_eq!(parse("22 ラウンド終了 1 い Ⅱ 』"), Some(22));
        assert_eq!(parse("25 ラウンド終了 1 い Ⅱ 』"), Some(25));
    }

    #[test]
    fn round_end_with_noise_before() {
        // Real OCR: ". 、 を - イ 4 当 y メ 新 ラウンド終了 に は"
        assert_eq!(parse("4 当 y メ 新 ラウンド終了 に は"), Some(4));
    }

    #[test]
    fn round_select_panel() {
        assert_eq!(parse("02 ラウンド"), Some(2));
        assert_eq!(parse("14 ラウンド"), Some(14));
    }

    #[test]
    fn ocr_with_spaces() {
        assert_eq!(parse("0 3 ラ ウ ン ド"), Some(3));
    }

    #[test]
    fn ocr_with_v_checkmark() {
        assert_eq!(parse("01 V ラウンド"), Some(1));
    }

    #[test]
    fn no_round_text() {
        assert_eq!(parse("damage 1234"), None);
    }

    #[test]
    fn round_text_but_no_number_before() {
        assert_eq!(parse("ラウンド"), None);
        assert_eq!(parse("ラウンド終了"), None);
    }

    #[test]
    fn three_digit_number_ignored() {
        assert_eq!(parse("123 ラウンド"), None);
    }

    #[test]
    fn round_00_invalid() {
        assert_eq!(parse("00 ラウンド"), None);
    }

    #[test]
    fn bonus_text_not_round_end() {
        // "13 ラウンド Bonus" — is_end=false so won't emit, but parse still works
        assert_eq!(parse("13 ラウンド Bonus"), Some(13));
    }

    // --- parse_select_header ---

    #[test]
    fn header_full_width_parens() {
        assert_eq!(parse_select_header("自動周回中（13/99）"), Some(13));
        assert_eq!(parse_select_header("自動周回中（1/5）"), Some(1));
    }

    #[test]
    fn header_half_width_parens() {
        assert_eq!(parse_select_header("自 動 周 回 中 ( 1 / 5 )"), Some(1));
        assert_eq!(parse_select_header("自 動 周 回 中 ( 13 / 99 )"), Some(13));
    }

    #[test]
    fn header_ocr_colon_variant() {
        // Real OCR: "自動周回中 : 98 ′ 9"  (（98/99） misread)
        assert_eq!(parse_select_header("自 動 周 回 中 : 98 ′ 9"), Some(98));
    }

    #[test]
    fn header_misread_x_does_not_return_y() {
        // OCR misreads X as kanji: "自動周回中(劉/99)" → should be None, not 99
        assert_eq!(parse_select_header("自 動 周 回 中 ( 劉 / 99 )"), None);
    }

    #[test]
    fn header_no_match() {
        assert_eq!(parse_select_header("ラウンド終了"), None);
        assert_eq!(parse_select_header("random text"), None);
    }

    // --- is_round_end_text ---

    #[test]
    fn detects_round_end() {
        assert!(is_round_end_text("01 ラウンド終了"));
        assert!(is_round_end_text("ラ ウ ン ド 終 了"));
    }

    #[test]
    fn not_round_end() {
        assert!(!is_round_end_text("02 ラウンド"));
        assert!(!is_round_end_text("random text"));
    }

    // --- is_round_select_text ---

    #[test]
    fn detects_round_select() {
        assert!(is_round_select_text(
            "自動周回中（1/5）、カウントダウン終了後に自動で続行します"
        ));
        assert!(is_round_select_text("自 動 周 回 中"));
    }

    #[test]
    fn not_round_select() {
        assert!(!is_round_select_text("01 ラウンド終了"));
    }
}
