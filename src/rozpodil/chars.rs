//! Character classification shared by the sentence splitter and the tokenizer.

/// A decoded character together with its byte span in the source string.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Cp {
    pub value: char,
    pub start: usize,
    pub stop: usize,
}

/// Decodes `text` into characters tagged with their byte spans.
pub(crate) fn codepoints(text: &str) -> Vec<Cp> {
    text.char_indices()
        .map(|(start, value)| Cp { value, start, stop: start + value.len_utf8() })
        .collect()
}

pub(crate) fn is_space(cp: char) -> bool {
    matches!(cp, ' ' | '\t' | '\n' | '\r' | '\u{0c}' | '\u{0b}' | '\u{a0}')
}

pub(crate) fn is_digit(cp: char) -> bool {
    cp.is_ascii_digit()
}

pub(crate) fn is_latin(cp: char) -> bool {
    cp.is_ascii_alphabetic()
}

pub(crate) fn is_uk(cp: char) -> bool {
    matches!(cp, 'а'..='я' | 'А'..='Я' | 'ґ' | 'Ґ' | 'є' | 'Є' | 'і' | 'І' | 'ї' | 'Ї')
}

pub(crate) fn is_alpha(cp: char) -> bool {
    is_latin(cp) || is_uk(cp)
}

pub(crate) fn is_uk_apostrophe(cp: char) -> bool {
    matches!(cp, '\'' | '\u{2019}' | '\u{02bc}' | '`')
}

pub(crate) fn is_word_mark(cp: char) -> bool {
    cp == '_' || cp == '-'
}

/// True when an apostrophe sits between two Ukrainian letters, as in `зв'язок`.
pub(crate) fn is_inner_uk_apostrophe(cps: &[Cp], index: usize) -> bool {
    index > 0
        && index + 1 < cps.len()
        && is_uk_apostrophe(cps[index].value)
        && is_uk(cps[index - 1].value)
        && is_uk(cps[index + 1].value)
}

/// The extra letters the reference implementation treats as word characters.
fn is_extra_letter(cp: char) -> bool {
    matches!(cp, 'Å' | 'å' | 'ğ' | 'Ğ') || ('\u{0370}'..='\u{03ff}').contains(&cp)
}

pub(crate) fn is_word_letter(cp: char) -> bool {
    is_alpha(cp) || cp == '_' || is_extra_letter(cp)
}

pub(crate) fn is_python_alpha(cp: char) -> bool {
    is_alpha(cp) || is_extra_letter(cp)
}

pub(crate) fn is_word_cp(cp: char) -> bool {
    is_word_letter(cp) || is_digit(cp)
}

/// Lowercases the subset of characters the reference implementation knows about,
/// leaving everything else untouched.
pub(crate) fn lower_cp(cp: char) -> char {
    match cp {
        'A'..='Z' | 'А'..='Я' | '\u{0391}'..='\u{03a9}' => {
            char::from_u32(cp as u32 + 32).unwrap_or(cp)
        }
        'Ґ' => 'ґ',
        'Є' => 'є',
        'І' => 'і',
        'Ї' => 'ї',
        'Å' => 'å',
        'Ğ' => 'ğ',
        _ => cp,
    }
}

/// Lowercases ASCII and Ukrainian letters only; other characters pass through.
pub(crate) fn lower_ascii_ukrainian(text: &str) -> String {
    text.chars()
        .map(|cp| match cp {
            'A'..='Z' | 'А'..='Я' => char::from_u32(cp as u32 + 32).unwrap_or(cp),
            'Ґ' => 'ґ',
            'Є' => 'є',
            'І' => 'і',
            'Ї' => 'ї',
            _ => cp,
        })
        .collect()
}

/// True when every character is a lowercase letter and there is at least one.
pub(crate) fn is_lower_alpha(token: &str) -> bool {
    !token.is_empty() && token.chars().all(|cp| is_python_alpha(cp) && lower_cp(cp) == cp)
}

/// True when the token is exactly one uppercase letter.
pub(crate) fn is_upper_one(token: &str) -> bool {
    let mut chars = token.chars();
    match (chars.next(), chars.next()) {
        (Some(cp), None) => is_python_alpha(cp) && lower_cp(cp) != cp,
        _ => false,
    }
}

/// Matches an emoticon such as `:-)` or `=(((` starting at byte `pos`.
///
/// Returns the byte offset just past the emoticon.
pub(crate) fn smile_at(text: &str, pos: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    if pos >= bytes.len() || !matches!(bytes[pos], b'=' | b':' | b';') {
        return None;
    }
    let mut i = pos + 1;
    if bytes.get(i) == Some(&b'-') {
        i += 1;
    }
    let mut count = 0;
    while count < 3 && matches!(bytes.get(i), Some(b'(' | b')')) {
        i += 1;
        count += 1;
    }
    (count > 0).then_some(i)
}
