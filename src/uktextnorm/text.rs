//! Character classification and small string utilities.

/// Ukrainian letters, plus the apostrophes that bind a word together.
pub(crate) fn is_uk(cp: char) -> bool {
    matches!(cp, 'а'..='я' | 'А'..='Я' | 'є' | 'Є' | 'і' | 'І' | 'ї' | 'Ї' | 'ґ' | 'Ґ' | '’' | '\'')
}

pub(crate) fn is_upper_uk(cp: char) -> bool {
    matches!(cp, 'А'..='Я' | 'Є' | 'І' | 'Ї' | 'Ґ')
}

pub(crate) fn is_latin(cp: char) -> bool {
    cp.is_ascii_alphabetic()
}

pub(crate) fn is_word_joiner(cp: char) -> bool {
    matches!(cp, '\'' | '’' | '–' | '-')
}

/// Lowercases ASCII and Ukrainian letters; everything else passes through.
pub(crate) fn lower_cp(cp: char) -> char {
    match cp {
        'A'..='Z' | 'А'..='Я' => char::from_u32(cp as u32 + 32).unwrap_or(cp),
        'Є' => 'є',
        'І' => 'і',
        'Ї' => 'ї',
        'Ґ' => 'ґ',
        _ => cp,
    }
}

/// Uppercases ASCII and Ukrainian letters; everything else passes through.
pub(crate) fn upper_cp(cp: char) -> char {
    match cp {
        'a'..='z' | 'а'..='я' => char::from_u32(cp as u32 - 32).unwrap_or(cp),
        'є' => 'Є',
        'і' => 'І',
        'ї' => 'Ї',
        'ґ' => 'Ґ',
        _ => cp,
    }
}

pub(crate) fn lower_text(text: &str) -> String {
    text.chars().map(lower_cp).collect()
}

/// Uppercases the first ASCII-lowercase or Ukrainian letter in `text`.
pub(crate) fn capitalize_first_letter(text: &str) -> String {
    for (i, cp) in text.char_indices() {
        if cp.is_ascii_lowercase() || is_uk(cp) {
            let mut out = String::with_capacity(text.len());
            out.push_str(&text[..i]);
            out.push(upper_cp(cp));
            out.push_str(&text[i + cp.len_utf8()..]);
            return out;
        }
    }
    text.to_owned()
}

/// Drops whitespace and lowercases, so lookup keys ignore spacing (`т. д.` -> `т.д.`).
pub(crate) fn compact_spaces_lower(text: &str) -> String {
    text.chars().filter(|cp| !matches!(cp, ' ' | '\t' | '\n' | '\r')).map(lower_cp).collect()
}

/// Uppercases and keeps only ASCII alphanumerics, for checksum validation.
pub(crate) fn compact_ascii_alnum_upper(value: &str) -> String {
    value.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_uppercase()).collect()
}

pub(crate) fn has_ascii_digit(text: &str) -> bool {
    text.bytes().any(|b| b.is_ascii_digit())
}

pub(crate) fn has_ascii_alpha(text: &str) -> bool {
    text.bytes().any(|b| b.is_ascii_alphabetic())
}

pub(crate) fn contains_any(text: &str, chars: &str) -> bool {
    text.chars().any(|cp| chars.contains(cp))
}

pub(crate) fn contains_any_token(text: &str, tokens: &[&str]) -> bool {
    tokens.iter().any(|token| text.contains(token))
}

pub(crate) fn is_ascii_acronym(text: &str) -> bool {
    (2..=6).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_uppercase())
}

/// Collapses runs of spaces and trims the ends.
pub(crate) fn trim_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut previous_space = false;
    for ch in text.chars() {
        if ch == ' ' {
            if !previous_space {
                out.push(ch);
            }
            previous_space = true;
        } else {
            out.push(ch);
            previous_space = false;
        }
    }
    out.trim_matches(' ').to_owned()
}

/// Splits on runs of ASCII whitespace, as `std::istringstream >> word` does.
pub(crate) fn split_words(text: &str) -> Vec<String> {
    text.split_ascii_whitespace().map(str::to_owned).collect()
}

pub(crate) fn join(words: &[String]) -> String {
    words.join(" ")
}

/// Parses a whole non-negative integer, or `None` when `text` is not one.
pub(crate) fn try_parse_u64(text: &str) -> Option<u64> {
    (!text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

/// Parses a leading non-negative integer, yielding 0 when there is none.
///
/// This mirrors `std::from_chars`, which the reference implementation uses on
/// values the surrounding regex has already constrained to digits.
pub(crate) fn parse_u64(text: &str) -> u64 {
    let end = text.bytes().take_while(u8::is_ascii_digit).count();
    text[..end].parse().unwrap_or(0)
}

pub(crate) fn parse_i32(text: &str) -> i32 {
    let digits = text.strip_prefix('-').unwrap_or(text);
    let end = digits.bytes().take_while(u8::is_ascii_digit).count();
    let value: i32 = digits[..end].parse().unwrap_or(0);
    if text.starts_with('-') {
        -value
    } else {
        value
    }
}

/// A word span in the source text, used when reporting uncertainty.
pub(crate) struct WordSpan {
    pub start: usize,
    pub stop: usize,
}

pub(crate) fn is_uncertain_word_char(cp: char) -> bool {
    is_latin(cp) || is_uk(cp) || is_word_joiner(cp)
}

/// Finds maximal runs of word characters that contain at least one letter.
pub(crate) fn uncertain_word_spans(text: &str) -> Vec<WordSpan> {
    let mut spans = Vec::new();
    let mut start = None;
    let mut stop = 0;
    let mut has_letter = false;
    let mut flush = |start: &mut Option<usize>, stop: usize, has_letter: &mut bool| {
        if let Some(s) = start.take() {
            if *has_letter {
                spans.push(WordSpan { start: s, stop });
            }
        }
        *has_letter = false;
    };
    for (i, cp) in text.char_indices() {
        if is_uncertain_word_char(cp) {
            start.get_or_insert(i);
            stop = i + cp.len_utf8();
            has_letter |= is_latin(cp) || (is_uk(cp) && !is_word_joiner(cp));
        } else {
            flush(&mut start, stop, &mut has_letter);
        }
    }
    flush(&mut start, stop, &mut has_letter);
    spans
}

pub(crate) fn has_roman_candidate(text: &str) -> bool {
    text.bytes().any(|b| b"MDCLXVI".contains(&b))
}

pub(crate) fn has_symbol_candidate(text: &str) -> bool {
    contains_any_token(
        text,
        &[
            "°", "℃", "℉", "\u{212a}", "±", "≈", "≠", "≤", "≥", "×", "÷", "=", "<", ">", "‰", "§",
            "₿", "•", "·", "~", "&", "#", "_", "²", "³", "№",
        ],
    )
}

/// Escapes the characters that are special inside a regex alternation.
fn escape_alternation(key: &str) -> String {
    let mut out = String::with_capacity(key.len());
    for ch in key.chars() {
        if r"\-[]{}()*+?.,^$|# ".contains(ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Builds a regex alternation with the longest keys first, so that the leftmost
/// alternative that matches is also the longest one.
pub(crate) fn regex_alternation<I>(keys: I) -> String
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    let mut keys: Vec<String> = keys.into_iter().map(|k| k.as_ref().to_owned()).collect();
    keys.sort_by_key(|k| std::cmp::Reverse(k.len()));
    keys.iter().map(|k| escape_alternation(k)).collect::<Vec<_>>().join("|")
}
