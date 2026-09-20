//! Abbreviation expansion, acronym spelling and Latin transliteration.

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::uktextnorm::lexicon;
use crate::uktextnorm::morphology::{PRONUNCIATION, TRANSLITERATION};
use crate::uktextnorm::readers::ABBREVIATIONS;
use crate::uktextnorm::text::{
    capitalize_first_letter, compact_spaces_lower, is_latin, is_uk, is_upper_uk, join, lower_cp,
    lower_text,
};

/// True for the characters that make an abbreviation need a word boundary.
fn is_word_character(cp: char) -> bool {
    is_uk(cp) || is_latin(cp) || cp.is_ascii_digit()
}

/// The character ending at byte `index`, when there is one.
fn char_before(text: &str, index: usize) -> Option<char> {
    text[..index].chars().next_back()
}

/// Tries to match `key` against `text` starting at `start`.
///
/// Spaces in a key match any run of spaces (including none) and a period in a
/// key may be preceded by spaces, so `т.д.`, `т. д.` and `т . д .` all match
/// the key `т. д.`. Returns the byte offset just past the match.
fn match_key_at(text: &str, start: usize, key: &str) -> Option<usize> {
    let skip_spaces = |pos: &mut usize| {
        while text[*pos..].starts_with(' ') {
            *pos += 1;
        }
    };
    let mut pos = start;
    let mut rest = key;
    while let Some(kc) = rest.chars().next() {
        match kc {
            ' ' => {
                skip_spaces(&mut pos);
                rest = &rest[1..];
            }
            '.' => {
                skip_spaces(&mut pos);
                if !text[pos..].starts_with('.') {
                    return None;
                }
                pos += 1;
                rest = &rest[1..];
                if !rest.is_empty() {
                    skip_spaces(&mut pos);
                }
            }
            _ => {
                let tc = text[pos..].chars().next()?;
                if lower_cp(kc) != lower_cp(tc) {
                    return None;
                }
                pos += tc.len_utf8();
                rest = &rest[kc.len_utf8()..];
            }
        }
    }
    Some(pos)
}

/// Expands the abbreviations listed in the lexicon.
///
/// ```
/// # use normalize_uk::uktextnorm::normalize_abbreviations;
/// assert_eq!(normalize_abbreviations("вул. Хрещатик"), "вулиця Хрещатик");
/// ```
pub fn normalize_abbreviations(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    'outer: while i < text.len() {
        // The lexicon is ordered so that longer overlapping keys come first.
        for &(key, _) in lexicon::ABBREVIATIONS.iter() {
            let Some(pos) = match_key_at(text, i, key) else { continue };
            let left_boundary = char_before(text, i).is_none_or(|cp| !is_word_character(cp));
            let Some(key_start) = key.chars().next() else { continue };
            let Some(key_end) = key.chars().next_back() else { continue };
            let following = text[pos..].chars().next();
            let right_boundary =
                !is_word_character(key_end) || following.is_none_or(|cp| !is_word_character(cp));
            if (is_word_character(key_start) && !left_boundary) || !right_boundary {
                continue;
            }
            let Some(&expansion) = ABBREVIATIONS.get(&compact_spaces_lower(&text[i..pos])) else {
                continue;
            };
            let Some(first) = text[i..].chars().next() else { break 'outer };
            if is_upper_uk(first) {
                out.push_str(&capitalize_first_letter(expansion));
            } else {
                out.push_str(expansion);
            }
            // A key ending in a period that closes the text keeps its period,
            // which doubles as the sentence's full stop.
            if key.ends_with('.') && pos == text.len() {
                out.push('.');
            }
            i = pos;
            continue 'outer;
        }
        let Some(cp) = text[i..].chars().next() else { break };
        out.push(cp);
        i += cp.len_utf8();
    }
    out
}

/// Spells out all-caps Cyrillic acronyms that have no vowel, letter by letter.
///
/// ```
/// # use normalize_uk::uktextnorm::expand_abbreviations;
/// // A vowel makes the run a word, so НАТО is left alone.
/// assert_eq!(expand_abbreviations("СБР і НАТО"), "ес бе ер і НАТО");
/// ```
pub fn expand_abbreviations(text: &str) -> String {
    const VOWELS: &str = "АЕЄИІЇОУЮЯ";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(cp) = rest.chars().next() {
        if !is_upper_uk(cp) {
            out.push(cp);
            rest = &rest[cp.len_utf8()..];
            continue;
        }
        let run_len = rest.chars().take_while(|&c| is_upper_uk(c)).map(char::len_utf8).sum();
        let (token, tail) = rest.split_at(run_len);
        rest = tail;
        // A single letter is an initial, and a run with a vowel is a word.
        if token.chars().count() < 2 || token.chars().any(|c| VOWELS.contains(c)) {
            out.push_str(token);
            continue;
        }
        let parts: Vec<String> = token
            .chars()
            .filter_map(|c| PRONUNCIATION.get(c.to_string().as_str()).map(|&s| s.to_owned()))
            .collect();
        out.push_str(&join(&parts));
    }
    out
}

/// Latin letters carrying diacritics, mapped to their nearest Cyrillic reading.
static LATIN_DIACRITICS: LazyLock<HashMap<char, &'static str>> = LazyLock::new(|| {
    let groups: [(&str, &str); 11] = [
        ("áàâãåāăąÁÀÂÃÅĀĂĄ", "а"),
        ("äÄéèêëēėęÉÈÊËĒĖĘ", "е"),
        ("íìîïīÍÌÎÏĪ", "і"),
        ("óòôõöōÓÒÔÕÖŌ", "о"),
        ("úùûūÚÙÛŪ", "у"),
        ("üÜ", "ю"),
        ("çÇ", "с"),
        ("ñÑ", "нь"),
        ("ß", "сс"),
        ("łŁ", "л"),
        ("ýÿÝŸ", "и"),
    ];
    groups.iter().flat_map(|&(chars, to)| chars.chars().map(move |c| (c, to))).collect()
});

/// Rewrites Latin script as Cyrillic, longest letter sequences first.
///
/// ```
/// # use normalize_uk::uktextnorm::transliterate_to_cyrillic;
/// assert_eq!(transliterate_to_cyrillic("shash"), "шаш");
/// ```
pub fn transliterate_to_cyrillic(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let Some(cp) = text[i..].chars().next() else {
            break;
        };
        if cp.is_ascii_alphabetic() {
            // Try three-, then two-, then one-letter sequences.
            let matched = [3usize, 2, 1].into_iter().find_map(|len| {
                let end = i + len;
                if end > text.len() || !text.is_char_boundary(end) {
                    return None;
                }
                let key = lower_text(&text[i..end]);
                TRANSLITERATION.get(key.as_str()).map(|&value| (len, value))
            });
            if let Some((len, value)) = matched {
                out.push_str(value);
                i += len;
            } else {
                out.push(cp);
                i += cp.len_utf8();
            }
            continue;
        }
        match LATIN_DIACRITICS.get(&cp) {
            Some(value) => out.push_str(value),
            None => out.push(cp),
        }
        i += cp.len_utf8();
    }
    out
}
