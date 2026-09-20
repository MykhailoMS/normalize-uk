//! Sentence segmentation.

use super::abbrev;
use super::chars::{
    codepoints, is_alpha, is_digit, is_lower_alpha, is_space, is_upper_one, is_word_cp,
    is_word_letter, lower_ascii_ukrainian, smile_at, Cp,
};
use super::{push_substring, Substring};

const ENDINGS: &str = ".?!…";
const DASHES: &str = "‑–—−-";
const GENERIC_QUOTES: &str = "\"„'";
const CLOSE_QUOTES: &str = "»”’";
const CLOSE_BRACKETS: &str = ")]}";
const DELIMITERS: &str = ".?!…;\"„'»”’)]}";

/// What to do at a candidate sentence boundary.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    /// Split here.
    Split,
    /// Keep the two halves in one sentence.
    Join,
}

/// The window of context around a candidate boundary.
struct SentSplit<'a> {
    left: &'a str,
    delimiter: &'a str,
    right: &'a str,
    buffer: &'a str,
}

fn first_char(text: &str) -> Option<char> {
    text.chars().next()
}

fn starts_with_space(text: &str) -> bool {
    first_char(text).is_some_and(is_space)
}

fn ends_with_space(text: &str) -> bool {
    text.chars().next_back().is_some_and(is_space)
}

/// Returns the byte offset of the first non-space character and the trimmed slice.
pub(crate) fn trim_view(text: &str) -> (usize, &str) {
    let start = text.find(|c: char| !is_space(c)).unwrap_or(text.len());
    let end = text
        .rfind(|c: char| !is_space(c))
        .map_or(start, |i| i + text[i..].chars().next().map_or(0, char::len_utf8));
    (start, &text[start..end])
}

/// Splits `text` into maximal runs of word letters, of digits, and single other
/// characters, skipping whitespace.
fn tokens_in(text: &str) -> Vec<&str> {
    let cps = codepoints(text);
    let mut out = Vec::new();
    let mut i = 0;
    while i < cps.len() {
        if is_space(cps[i].value) {
            i += 1;
            continue;
        }
        let begin = i;
        if is_word_letter(cps[i].value) {
            while i < cps.len() && is_word_letter(cps[i].value) {
                i += 1;
            }
        } else if is_digit(cps[i].value) {
            while i < cps.len() && is_digit(cps[i].value) {
                i += 1;
            }
        } else {
            i += 1;
        }
        out.push(&text[cps[begin].start..cps[i - 1].stop]);
    }
    out
}

/// The first token of `text`: a run of word letters, a run of digits, or one
/// other character.
fn first_token(text: &str) -> Option<&str> {
    let cps = codepoints(text);
    for i in 0..cps.len() {
        if is_space(cps[i].value) {
            continue;
        }
        return Some(run_forward(text, &cps, i));
    }
    None
}

/// The first run of word letters or digits, ignoring everything else.
fn first_word(text: &str) -> Option<&str> {
    let cps = codepoints(text);
    for i in 0..cps.len() {
        if is_word_letter(cps[i].value) || is_digit(cps[i].value) {
            return Some(run_forward(text, &cps, i));
        }
    }
    None
}

fn run_forward<'a>(text: &'a str, cps: &[Cp], i: usize) -> &'a str {
    let pred: fn(char) -> bool = if is_word_letter(cps[i].value) {
        is_word_letter
    } else if is_digit(cps[i].value) {
        is_digit
    } else {
        return &text[cps[i].start..cps[i].stop];
    };
    let mut j = i + 1;
    while j < cps.len() && pred(cps[j].value) {
        j += 1;
    }
    &text[cps[i].start..cps[j - 1].stop]
}

/// The last token of `text`, mirroring [`first_token`].
fn last_token(text: &str) -> Option<&str> {
    let cps = codepoints(text);
    for i in (0..cps.len()).rev() {
        if is_space(cps[i].value) {
            continue;
        }
        let pred: fn(char) -> bool = if is_word_letter(cps[i].value) {
            is_word_letter
        } else if is_digit(cps[i].value) {
            is_digit
        } else {
            return Some(&text[cps[i].start..cps[i].stop]);
        };
        let mut j = i;
        while j > 0 && pred(cps[j - 1].value) {
            j -= 1;
        }
        return Some(&text[cps[j].start..cps[i].stop]);
    }
    None
}

/// The trailing token when it contains an inner `-`, `.` or `/`, as in `чл.-кор`.
fn last_compound_abbrev_token(text: &str) -> Option<&str> {
    let cps = codepoints(text);
    for i in (0..cps.len()).rev() {
        if is_space(cps[i].value) {
            continue;
        }
        if !is_word_cp(cps[i].value) {
            return None;
        }
        let joiner = |c: char| matches!(c, '-' | '.' | '/');
        let mut saw_compound_mark = false;
        let mut j = i;
        while j > 0 && (is_word_cp(cps[j - 1].value) || joiner(cps[j - 1].value)) {
            saw_compound_mark |= joiner(cps[j - 1].value);
            j -= 1;
        }
        while j <= i && joiner(cps[j].value) {
            j += 1;
        }
        if j > i || !saw_compound_mark {
            return None;
        }
        return Some(&text[cps[j].start..cps[i].stop]);
    }
    None
}

/// The word before a trailing period, as in `... тис.` -> `тис`.
fn trailing_dot_abbrev_token(text: &str) -> Option<&str> {
    let cps = codepoints(text);
    let mut i = cps.len();
    while i > 0 && is_space(cps[i - 1].value) {
        i -= 1;
    }
    if i == 0 || cps[i - 1].value != '.' {
        return None;
    }
    i -= 1;
    while i > 0 && is_space(cps[i - 1].value) {
        i -= 1;
    }
    let stop = i;
    while i > 0 && is_word_cp(cps[i - 1].value) {
        i -= 1;
    }
    (i != stop).then(|| &text[cps[i].start..cps[stop - 1].stop])
}

/// Splits a trailing `<word>. <word>` into its two words, as in `т. д`.
fn left_abbreviation_pair(text: &str) -> Option<(&str, &str)> {
    let cps = codepoints(text);
    let mut i = cps.len();
    while i > 0 && is_space(cps[i - 1].value) {
        i -= 1;
    }
    let b_stop = i;
    while i > 0 && is_word_cp(cps[i - 1].value) {
        i -= 1;
    }
    if i == b_stop {
        return None;
    }
    let b_start = i;
    while i > 0 && is_space(cps[i - 1].value) {
        i -= 1;
    }
    if i == 0 || cps[i - 1].value != '.' {
        return None;
    }
    i -= 1;
    while i > 0 && is_space(cps[i - 1].value) {
        i -= 1;
    }
    let a_stop = i;
    while i > 0 && is_word_cp(cps[i - 1].value) {
        i -= 1;
    }
    if i == a_stop {
        return None;
    }
    Some((
        &text[cps[i].start..cps[a_stop - 1].stop],
        &text[cps[b_start].start..cps[b_stop - 1].stop],
    ))
}

fn starts_with_letter_bullet(text: &str) -> bool {
    let cps = codepoints(text);
    let mut i = 0;
    while i < cps.len() && is_space(cps[i].value) {
        i += 1;
    }
    if i + 1 >= cps.len() || !is_alpha(cps[i].value) || cps[i + 1].value != ')' {
        return false;
    }
    i + 2 >= cps.len() || is_space(cps[i + 2].value)
}

/// True when `token` can legitimately follow an abbreviation's period.
fn can_follow_abbreviation(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    if token.chars().all(is_digit) {
        return true;
    }
    if !token.chars().all(is_alpha) {
        return true;
    }
    is_lower_alpha(token)
}

fn is_roman_token(token: &str) -> bool {
    !token.is_empty() && token.chars().all(|c| "IVXLCDM".contains(c))
}

fn is_article_abbrev_right(token: &str) -> bool {
    !token.is_empty() && (token.chars().all(is_digit) || is_roman_token(token))
}

fn is_bullet(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    if token.bytes().all(|b| b.is_ascii_digit()) || token == "." || token == ")" {
        return true;
    }
    let lower = lower_ascii_ukrainian(token);
    "§абвгдеabcdef".contains(&lower) || (token.chars().all(|c| "IVXML".contains(c)))
}

fn smile_prefix(text: &str) -> bool {
    let offset = text.find(|c: char| !is_space(c)).unwrap_or(text.len());
    smile_at(text, offset).is_some()
}

fn sent_join(split: &SentSplit<'_>) -> Action {
    let (Some(left), Some(right)) = (last_token(split.left), first_token(split.right)) else {
        return Action::Join;
    };
    if !starts_with_space(split.right) {
        return Action::Join;
    }

    let first_non_space = split.right.find(|c: char| !matches!(c, ' ' | '\t' | '\r' | '\n'));
    let leading_space = &split.right[..first_non_space.unwrap_or(split.right.len())];

    // A wiki-style `== heading ==` never ends mid-way.
    let current_line = match split.buffer.rfind(['\r', '\n']) {
        Some(i) => &split.buffer[i + 1..],
        None => split.buffer,
    };
    let heading = current_line
        .find(|c: char| !matches!(c, ' ' | '\t'))
        .is_some_and(|i| current_line[i..].starts_with("=="));
    if heading && first_non_space.is_some_and(|i| split.right[i..].starts_with("==")) {
        return Action::Join;
    }

    if leading_space.contains(['\n', '\r']) {
        return Action::Split;
    }
    if starts_with_letter_bullet(split.right) {
        return Action::Split;
    }
    if is_lower_alpha(right) {
        return Action::Join;
    }

    let right_cp = first_char(right).unwrap_or('\0');
    if !GENERIC_QUOTES.contains(right_cp)
        && (DELIMITERS.contains(right_cp) || smile_prefix(split.right))
    {
        return Action::Join;
    }

    let delimiter_cp = first_char(split.delimiter).unwrap_or('\0');
    let left_lower = lower_ascii_ukrainian(last_compound_abbrev_token(split.left).unwrap_or(left));

    if split.delimiter == "." {
        // `буд. 5 м. Київ`: a measurement abbreviation after a number is not a boundary.
        if left_lower == "м" || left_lower == "с" {
            let tokens = tokens_in(split.left);
            if tokens.len() >= 2 && tokens[tokens.len() - 2].chars().all(is_digit) {
                return Action::Split;
            }
        }
        if left.chars().all(is_digit) && right.chars().all(is_digit) {
            return Action::Join;
        }
        if let Some(dotted) = trailing_dot_abbrev_token(split.left) {
            let dotted_lower = lower_ascii_ukrainian(dotted);
            if abbrev::is_known(&dotted_lower) && can_follow_abbreviation(right) {
                return Action::Join;
            }
        }
        let mut skip_single_abbreviation = false;
        if let Some((a, b)) = left_abbreviation_pair(split.left) {
            let pair = format!("{} {}", lower_ascii_ukrainian(a), lower_ascii_ukrainian(b));
            if abbrev::LEADING_PAIRS.contains(pair.as_str()) {
                return Action::Join;
            }
            if abbrev::PAIRS.contains(pair.as_str()) {
                if can_follow_abbreviation(right) {
                    return Action::Join;
                }
                skip_single_abbreviation = true;
            }
        }
        if !skip_single_abbreviation {
            let known = if left_lower == "ст" {
                is_article_abbrev_right(right)
            } else {
                abbrev::LEADING.contains(left_lower.as_str())
            };
            if known || (abbrev::is_known(&left_lower) && can_follow_abbreviation(right)) {
                return Action::Join;
            }
            let pair = format!("{} {}", left_lower, lower_ascii_ukrainian(right));
            if abbrev::PAIRS.contains(pair.as_str()) {
                return Action::Join;
            }
        }
        if is_upper_one(left) || abbrev::INITIALS.contains(left_lower.as_str()) {
            return Action::Join;
        }
    }

    // A short run of bullet markers such as `1.` or `а)` is a list item, not a sentence.
    if (split.delimiter == "." || split.delimiter == ")") && split.buffer.len() <= 20 {
        let toks = tokens_in(split.buffer);
        if !toks.is_empty() && toks.iter().all(|t| is_bullet(t)) {
            return Action::Join;
        }
    }

    if CLOSE_QUOTES.contains(delimiter_cp)
        || GENERIC_QUOTES.contains(delimiter_cp)
        || CLOSE_BRACKETS.contains(delimiter_cp)
    {
        let left_cp = first_char(left).unwrap_or('\0');
        if !ENDINGS.contains(left_cp) {
            return Action::Join;
        }
        if GENERIC_QUOTES.contains(delimiter_cp) && ends_with_space(split.left) {
            return Action::Join;
        }
    }

    if DASHES.contains(right_cp) && first_word(split.right).is_some_and(is_lower_alpha) {
        return Action::Join;
    }

    Action::Split
}

/// Splits `text` into sentences.
pub fn split_sentences(text: &str) -> Vec<Substring<'_>> {
    if text.chars().all(is_space) {
        return Vec::new();
    }
    let cps = codepoints(text);
    let mut out = Vec::new();
    let mut current_start = 0;
    let mut ci = 0;
    while ci < cps.len() {
        let mut stop = cps[ci].stop;
        let mut is_delim = DELIMITERS.contains(cps[ci].value);
        let mut delimiter_end_index = ci + 1;
        if let Some(smile_stop) = smile_at(text, cps[ci].start) {
            stop = smile_stop;
            is_delim = true;
            while delimiter_end_index < cps.len() && cps[delimiter_end_index].start < stop {
                delimiter_end_index += 1;
            }
        }
        if !is_delim {
            ci += 1;
            continue;
        }

        let start = cps[ci].start;
        // The decision only looks at ten characters on either side.
        let left_start = if ci > 10 { cps[ci - 10].start } else { 0 };
        let right_stop = if delimiter_end_index < cps.len() {
            cps[cps.len().min(delimiter_end_index + 10) - 1].stop
        } else {
            text.len()
        };
        let split = SentSplit {
            left: &text[left_start..start],
            delimiter: &text[start..stop],
            right: &text[stop..right_stop],
            buffer: &text[current_start..start],
        };
        if sent_join(&split) != Action::Join {
            push_substring(&mut out, text, current_start, stop, true);
            current_start = stop;
        }
        ci = delimiter_end_index;
    }
    push_substring(&mut out, text, current_start, text.len(), true);
    out
}
