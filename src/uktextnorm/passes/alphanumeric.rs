//! Mixed letter-and-digit tokens: English words, technical tokens and Cyrillic
//! identifiers such as `КС-19`.

use std::collections::HashMap;
use std::sync::LazyLock;

use fancy_regex::Regex;

use crate::uktextnorm::numbers::{number_to_words, number_to_words_digit_by_digit, ordinal_words};
use crate::uktextnorm::re::{cap, compile, compile_i, sub, whole};
use crate::uktextnorm::readers::{read_identifier_number, spell_identifier_letters, ENGLISH_WORDS};
use crate::uktextnorm::text::{
    is_uk, is_upper_uk, is_word_joiner, join, lower_text, try_parse_u64,
};
use crate::uktextnorm::{fuzzy_match, InputTolerance};

/// How each Latin letter is named when read aloud in Ukrainian.
#[rustfmt::skip]
static LATIN_LETTER_NAMES: LazyLock<HashMap<char, &'static str>> = LazyLock::new(|| {
    [
        ('a', "ей"), ('b', "бі"), ('c', "сі"), ('d', "ді"), ('e', "і"), ('f', "еф"),
        ('g', "джі"), ('h', "ейч"), ('i', "ай"), ('j', "джей"), ('k', "кей"), ('l', "ел"),
        ('m', "ем"), ('n', "ен"), ('o', "оу"), ('p', "пі"), ('q', "к'ю"), ('r', "ар"),
        ('s', "ес"), ('t', "ті"), ('u', "ю"), ('v', "ві"), ('w', "дабл ю"), ('x', "екс"),
        ('y', "вай"), ('z', "зед"),
    ]
    .into_iter()
    .collect()
});

fn spell_latin_run(run: &str) -> String {
    let parts: Vec<String> = run
        .chars()
        .filter_map(|c| LATIN_LETTER_NAMES.get(&c.to_ascii_lowercase()).map(|&s| s.to_owned()))
        .collect();
    join(&parts)
}

/// Reads a run of digits, keeping leading zeroes audible.
fn read_ascii_digit_run(run: &str) -> String {
    if run.len() > 1 && run.starts_with('0') {
        return number_to_words_digit_by_digit(run);
    }
    match try_parse_u64(run) {
        Some(value) => number_to_words(value),
        None => number_to_words_digit_by_digit(run),
    }
}

/// Replaces known English words with their Ukrainian reading and spells out
/// unknown all-caps Latin acronyms.
///
/// Under [`InputTolerance::Asr`], a Latin word that misses the lexicon exactly
/// is retried through [`fuzzy_match::resolve`], so a recognizer's typo
/// (`spotifay`) still reaches its reading. Strict tolerance keeps the exact
/// behaviour.
pub(crate) fn normalize_english(
    text: &str,
    vocabulary: &HashMap<String, String>,
    tolerance: InputTolerance,
) -> String {
    static WORD: LazyLock<Regex> = LazyLock::new(|| compile(r"\b[A-Za-z][A-Za-z'’-]*\b"));
    static ACRONYM: LazyLock<Regex> = LazyLock::new(|| compile(r"\b[A-Z]+\b"));
    let text = sub(text, &WORD, |m| {
        let low = lower_text(whole(m));
        if let Some(reading) = vocabulary.get(&low) {
            return reading.clone();
        }
        if let Some(reading) = ENGLISH_WORDS.get(low.as_str()) {
            return (*reading).to_owned();
        }
        if tolerance == InputTolerance::Asr {
            // Miss: retry against the closed lexicon, tolerating ASR noise.
            let entries = ENGLISH_WORDS.iter().map(|(&k, &v)| (k, v));
            if let Some((reading, _kind)) = fuzzy_match::resolve(&low, entries) {
                return reading.to_owned();
            }
        }
        whole(m).to_owned()
    });
    sub(&text, &ACRONYM, |m| {
        let low = lower_text(whole(m));
        if vocabulary.contains_key(&low) || ENGLISH_WORDS.contains_key(low.as_str()) {
            return whole(m).to_owned();
        }
        spell_latin_run(&low)
    })
}

/// Reads technical tokens such as `IPv6`, `5G`, `3D`, `x86` and `21st`, then
/// splits any remaining mixed alphanumeric token into its runs.
pub(crate) fn normalize_technical_alphanumeric(text: &str) -> String {
    static INTERNET_PROTOCOL: LazyLock<Regex> = LazyLock::new(|| compile_i(r"\bIPv([46])\b"));
    static MOBILE_GENERATION: LazyLock<Regex> = LazyLock::new(|| compile(r"\b(\d+)G\b"));
    static DIMENSION: LazyLock<Regex> = LazyLock::new(|| compile(r"\b(\d+)D\b"));
    static X86_FAMILY: LazyLock<Regex> = LazyLock::new(|| compile_i(r"\bx(86|64)\b"));
    static ENGLISH_ORDINAL: LazyLock<Regex> =
        LazyLock::new(|| compile_i(r"\b(\d+)(?:st|nd|rd|th)\b"));
    static MIXED: LazyLock<Regex> = LazyLock::new(|| compile(r"\b[A-Za-z0-9]+\b"));

    let text = sub(text, &INTERNET_PROTOCOL, |m| {
        format!("ай пі версії {}", read_ascii_digit_run(cap(m, 1)))
    });
    let text =
        sub(&text, &MOBILE_GENERATION, |m| format!("{} джі", read_ascii_digit_run(cap(m, 1))));
    let text = sub(&text, &DIMENSION, |m| format!("{} ді", read_ascii_digit_run(cap(m, 1))));
    let text = sub(&text, &X86_FAMILY, |m| format!("ікс {}", read_ascii_digit_run(cap(m, 1))));
    let text = sub(&text, &ENGLISH_ORDINAL, |m| match try_parse_u64(cap(m, 1)) {
        Some(value) => ordinal_words(value, "nom"),
        None => whole(m).to_owned(),
    });

    sub(&text, &MIXED, |m| {
        let token = whole(m);
        let has_letter = token.bytes().any(|b| b.is_ascii_alphabetic());
        let has_digit = token.bytes().any(|b| b.is_ascii_digit());
        if !has_letter || !has_digit {
            return token.to_owned();
        }
        let mut parts = Vec::new();
        let bytes = token.as_bytes();
        let mut start = 0;
        while start < bytes.len() {
            let digits = bytes[start].is_ascii_digit();
            let mut stop = start + 1;
            while stop < bytes.len() && bytes[stop].is_ascii_digit() == digits {
                stop += 1;
            }
            let run = &token[start..stop];
            parts.push(if digits {
                read_ascii_digit_run(run)
            } else if run.bytes().all(|b| b.is_ascii_uppercase()) {
                spell_latin_run(run)
            } else {
                run.to_owned()
            });
            start = stop;
        }
        join(&parts)
    })
}

/// True when `х` sits between two digits, where it means "by" rather than a letter.
fn is_dimension_sign(chars: &[(usize, char)], i: usize, start: usize, stop: usize) -> bool {
    chars[i].1 == 'х'
        && i > start
        && i + 1 < stop
        && chars[i - 1].1.is_ascii_digit()
        && chars[i + 1].1.is_ascii_digit()
}

/// Spells out Cyrillic-and-digit identifiers such as `КС-19` or `3х4`.
pub(crate) fn normalize_cyrillic_alphanumeric(text: &str) -> String {
    let is_token_character = |cp: char| {
        (is_uk(cp) && !is_word_joiner(cp))
            || cp.is_ascii_digit()
            || matches!(cp, '-' | '/' | '–' | '—')
    };
    let is_separator = |cp: char| matches!(cp, '-' | '/' | '–' | '—');

    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let end_of = |i: usize| chars[i].0 + chars[i].1.len_utf8();

    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut index = 0;
    while index < chars.len() {
        // A token never starts on a separator.
        if !is_token_character(chars[index].1) || is_separator(chars[index].1) {
            index += 1;
            continue;
        }
        let start = index;
        while index < chars.len() && is_token_character(chars[index].1) {
            index += 1;
        }
        let mut stop = index;
        while stop > start && is_separator(chars[stop - 1].1) {
            stop -= 1;
        }

        let has_digit = (start..stop).any(|i| chars[i].1.is_ascii_digit());
        let has_ukrainian = (start..stop).any(|i| is_uk(chars[i].1) && !is_word_joiner(chars[i].1));
        let has_uppercase = (start..stop).any(|i| is_upper_uk(chars[i].1));
        let has_dimension_sign = (start..stop).any(|i| is_dimension_sign(&chars, i, start, stop));
        if !has_digit || !has_ukrainian || (!has_uppercase && !has_dimension_sign) {
            continue;
        }

        out.push_str(&text[last..chars[start].0]);
        let mut parts: Vec<String> = Vec::new();
        let mut i = start;
        while i < stop {
            let cp = chars[i].1;
            if cp.is_ascii_digit() {
                let run_start = chars[i].0;
                while i < stop && chars[i].1.is_ascii_digit() {
                    i += 1;
                }
                parts.push(read_identifier_number(&text[run_start..end_of(i - 1)]));
            } else if matches!(cp, '-' | '–' | '—') {
                parts.push("дефіс".to_owned());
                i += 1;
            } else if cp == '/' {
                parts.push("слеш".to_owned());
                i += 1;
            } else if is_dimension_sign(&chars, i, start, stop) {
                parts.push("помножити на".to_owned());
                i += 1;
            } else {
                let run_start = chars[i].0;
                while i < stop
                    && is_uk(chars[i].1)
                    && !is_word_joiner(chars[i].1)
                    && !is_dimension_sign(&chars, i, start, stop)
                {
                    i += 1;
                }
                parts.push(spell_identifier_letters(&text[run_start..end_of(i - 1)]));
            }
        }
        out.push_str(&join(&parts));
        last = end_of(stop - 1);
        index = stop;
    }
    if last == 0 {
        return text.to_owned();
    }
    out.push_str(&text[last..]);
    out
}

/// Distorted Cyrillic *readings* of known brand and English words, mapped from
/// a folded canonical key to the canonical reading.
///
/// The lexicon values (`вотсап`, `гугл`, `ютуб`, `спотіфай`) are the readings
/// this crate emits. ASR delivers them mis-spelled (`ватсап`, `спотифай`) or
/// with dropped separators, and no exact rule then matches. This index lets the
/// ASR-tolerant pass fold such a token back to the canonical reading.
///
/// A canonical key shared by two different readings is dropped from the index:
/// an ambiguous distortion must stay untouched rather than resolve to an
/// arbitrary reading.
static CYRILLIC_READINGS: LazyLock<HashMap<String, &'static str>> = LazyLock::new(|| {
    let mut by_key: HashMap<String, Option<&'static str>> = HashMap::new();
    for &reading in ENGLISH_WORDS.values() {
        // Only readings that are genuinely Cyrillic words; skip anything that is
        // still Latin or too short to fuzzy-match safely.
        if reading.chars().count() < 4 || !reading.chars().any(is_uk) {
            continue;
        }
        let key = fuzzy_match::canonical_key(reading);
        by_key
            .entry(key)
            .and_modify(|slot| {
                if *slot != Some(reading) {
                    *slot = None; // collision: two readings share a key
                }
            })
            .or_insert(Some(reading));
    }
    by_key.into_iter().filter_map(|(k, v)| v.map(|reading| (k, reading))).collect()
});

/// Folds distorted Cyrillic readings of known words back to canonical form.
///
/// Runs only under [`InputTolerance::Asr`]. A Cyrillic token that already is a
/// canonical reading is left untouched (the fast path); otherwise it is
/// resolved against the closed [`CYRILLIC_READINGS`] index via the canonical
/// key and a bounded fuzzy match. Free Ukrainian prose is safe: the target set
/// is only the foreign-origin readings (`вотсап`, `ютуб`, …), the edit budget
/// is 1–2, and ties are left unresolved.
pub(crate) fn normalize_cyrillic_readings(text: &str, tolerance: InputTolerance) -> String {
    if tolerance != InputTolerance::Asr {
        return text.to_owned();
    }
    static WORD: LazyLock<Regex> =
        LazyLock::new(|| compile(r"[А-Яа-яЄєІіЇїҐґ][А-Яа-яЄєІіЇїҐґ'’`-]*"));
    sub(text, &WORD, |m| {
        let token = whole(m);
        let low = lower_text(token);
        // Already a canonical reading (or its exact lowercase): leave it.
        if CYRILLIC_READINGS.values().any(|&r| r == low) {
            return token.to_owned();
        }
        let entries = CYRILLIC_READINGS.iter().map(|(k, &v)| (k.as_str(), v));
        // `resolve` folds the token to its canonical key and matches against the
        // index keys, which are themselves canonical — so exact/canonical/fuzzy
        // all resolve here.
        match fuzzy_match::resolve(&low, entries) {
            Some((reading, fuzzy_match::MatchKind::Canonical | fuzzy_match::MatchKind::Fuzzy)) => {
                reading.to_owned()
            }
            _ => token.to_owned(),
        }
    })
}
