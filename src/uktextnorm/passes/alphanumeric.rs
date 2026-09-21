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
use crate::uktextnorm::{fuzzy_match, lexicon, InputTolerance};

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

/// Canonical Cyrillic surface forms that ASR distorts, mapped from a folded
/// canonical key to the exact surface form to restore.
///
/// The targets are deliberately the *closed, foreign-shaped* sets — the readings
/// this crate emits for brands and English words (`вотсап`, `ютуб`, `спотіфай`)
/// and acronym keys (`ПДВ`, `ЗСУ`). These are not ordinary Ukrainian words, so a
/// bounded fuzzy match against them cannot drag prose onto a reading.
///
/// Deliberately **excluded**: unit and counted-noun word forms (`кілометрів`,
/// `документів`). Those are real inflected words whose neighbours in running
/// text are also real words, so fuzzy-matching them would corrupt prose.
/// Repairing a distorted *ordinary* word is a spell-checking problem against an
/// open dictionary, not a closed-lexicon fallback, and is out of scope here.
///
/// A canonical key shared by two different surface forms is dropped: an
/// ambiguous distortion must stay untouched rather than resolve arbitrarily.
static ASR_TARGETS: LazyLock<HashMap<String, &'static str>> = LazyLock::new(|| {
    let mut by_key: HashMap<String, Option<&'static str>> = HashMap::new();
    let mut add = |surface: &'static str| {
        // Cyrillic only, and at least an initialism's worth of letters. Short
        // targets (acronyms) are still safe because `canonicalize_asr` accepts
        // only an exact canonical fold for them, never a fuzzy guess.
        if surface.chars().count() < 3 || !surface.chars().any(is_uk) {
            return;
        }
        let key = fuzzy_match::canonical_key(surface);
        by_key
            .entry(key)
            .and_modify(|slot| {
                if *slot != Some(surface) {
                    *slot = None; // collision: two surface forms share a key
                }
            })
            .or_insert(Some(surface));
    };
    // Brand and English readings (the values this crate emits).
    for &reading in ENGLISH_WORDS.values() {
        add(reading);
    }
    // Acronym keys, which are Cyrillic initialisms (ПДВ, ЗСУ, …). Restoring the
    // exact key lets the downstream acronym pass expand it.
    for &(acronym, _) in lexicon::ACRONYMS.iter() {
        add(acronym);
    }
    by_key.into_iter().filter_map(|(k, v)| v.map(|surface| (k, surface))).collect()
});

/// Folds distorted Cyrillic tokens back to a canonical surface form, so the
/// downstream passes see clean input.
///
/// Runs only under [`InputTolerance::Asr`], as a preprocessing step before the
/// main pipeline. A token that already is a canonical target is left untouched
/// (the fast path); otherwise it is resolved against the closed [`ASR_TARGETS`]
/// Acronyms keyed by the folded spelling of their letter names, so a phonetic
/// ASR rendering resolves back to the acronym.
///
/// A recognizer often writes an initialism as it sounds: `ПДВ` -> `педеве`
/// (пе-де-ве), `СБУ` -> `есбеу`. The letters themselves are gone, so neither the
/// exact rule nor a canonical/edit-distance fold over `ПДВ` can catch it. This
/// index maps `canonical_key("пе"+"де"+"ве") = "педеве"` back to `ПДВ`.
///
/// Collisions (two acronyms whose spellings fold together) are dropped.
static ASR_SPELLED_ACRONYMS: LazyLock<HashMap<String, &'static str>> = LazyLock::new(|| {
    use crate::uktextnorm::morphology::PRONUNCIATION;
    let mut by_key: HashMap<String, Option<&'static str>> = HashMap::new();
    for &(acronym, _) in lexicon::ACRONYMS.iter() {
        // Spell each letter by its Ukrainian name and glue the names together.
        let mut spelled = String::new();
        let mut ok = true;
        for cp in acronym.chars() {
            match PRONUNCIATION.get(cp.to_string().as_str()) {
                Some(name) => spelled.push_str(name),
                None => {
                    ok = false;
                    break;
                }
            }
        }
        // Only worth indexing when spelling it out actually lengthens it into a
        // word-like token (a two-letter acronym spelled out stays ambiguous).
        if !ok || spelled.chars().count() < 4 {
            continue;
        }
        let key = fuzzy_match::canonical_key(&spelled);
        by_key
            .entry(key)
            .and_modify(|slot| {
                if *slot != Some(acronym) {
                    *slot = None;
                }
            })
            .or_insert(Some(acronym));
    }
    by_key.into_iter().filter_map(|(k, v)| v.map(|acronym| (k, acronym))).collect()
});

/// Folds distorted Cyrillic tokens back to a canonical surface form, so the
/// downstream passes see clean input.
///
/// Runs only under [`InputTolerance::Asr`], as a preprocessing step before the
/// main pipeline. Three kinds of distortion are repaired against closed sets:
///
/// - **spelling / separators / confusables** — `пдв` -> `ПДВ`, `ватсап` ->
///   `вотсап` (exact canonical fold; safe at any length);
/// - **edit-distance** — `спотифай` -> `спотіфай` (only for targets ≥ 4 chars,
///   where an accidental collision with a short real word is unlikely);
/// - **phonetic acronym spelling** — `педеве` -> `ПДВ`, via
///   [`ASR_SPELLED_ACRONYMS`].
///
/// A token that already is a canonical target verbatim is left untouched. Free
/// Ukrainian prose is safe: every built-in target set is closed and
/// foreign-shaped, the edit budget is 1–2, and ties are left unresolved.
///
/// `user_vocabulary` is the universal extension point: any canonical Cyrillic
/// words the caller supplies (a domain glossary, or a full Ukrainian lexicon)
/// are repaired by the very same phonetic-key and bounded-edit rules, so the
/// mechanism scales from the built-in closed sets up to open vocabulary without
/// a different code path.
pub(crate) fn canonicalize_asr(
    text: &str,
    tolerance: InputTolerance,
    user_vocabulary: &[String],
) -> String {
    if tolerance != InputTolerance::Asr {
        return text.to_owned();
    }
    // Fold the caller's words to phonetic keys once per call, dropping any key
    // shared by two different words (ambiguous — left unresolved).
    let user_index: HashMap<String, &str> = {
        let mut by_key: HashMap<String, Option<&str>> = HashMap::new();
        for word in user_vocabulary {
            if word.chars().count() < 3 || !word.chars().any(is_uk) {
                continue;
            }
            let key = fuzzy_match::canonical_key(word);
            by_key
                .entry(key)
                .and_modify(|slot| {
                    if *slot != Some(word.as_str()) {
                        *slot = None;
                    }
                })
                .or_insert(Some(word.as_str()));
        }
        by_key.into_iter().filter_map(|(k, v)| v.map(|w| (k, w))).collect()
    };

    // First collapse multi-word targets across a sliding window of tokens, so a
    // reading a recognizer split or glued is rejoined before per-token repair.
    let text = join_multiword_targets(text, user_vocabulary);

    static WORD: LazyLock<Regex> =
        LazyLock::new(|| compile(r"[А-Яа-яЄєІіЇїҐґ][А-Яа-яЄєІіЇїҐґ'’`-]*"));
    sub(&text, &WORD, |m| {
        let token = whole(m);
        // A verbatim canonical target (same case) needs no repair.
        if ASR_TARGETS.values().any(|&s| s == token) || user_vocabulary.iter().any(|w| w == token) {
            return token.to_owned();
        }
        let low = lower_text(token);
        let needle = fuzzy_match::canonical_key(&low);

        // Built-in sets are matched by the phonetic key ONLY — an exact fold,
        // never an edit-distance guess. The built-ins (brand readings, acronym
        // keys and spellings) sit in open Ukrainian prose, so a fuzzy match here
        // would drag ordinary words onto them (`через` -> a brand, `день` -> an
        // acronym). The phonetic key already absorbs the real ASR distortions;
        // edit-distance is reserved for the caller's own vocabulary below.

        // Phonetic acronym spelling: `педеве` -> `ПДВ`, `есбеу` -> `СБУ`.
        if let Some(&acronym) = ASR_SPELLED_ACRONYMS.get(&needle) {
            return acronym.to_owned();
        }

        // Brand readings and acronym keys, by exact phonetic key. Matching is
        // by the phonetic key ONLY — no edit-distance guess against the built-in
        // sets, because they sit in open prose and a short target like `тест`
        // (test) is one edit from a real word like `текст`. The phonetic key
        // already folds the real ASR distortions (vowel confusions, akannya,
        // separators, doublings); edit-distance is reserved for the caller's
        // curated vocabulary below.
        if let Some(&surface) = ASR_TARGETS.get(&needle) {
            return surface.to_owned();
        }

        // The caller's own vocabulary is the universal extension point. Here an
        // edit-distance fold is allowed for longer words, because the caller
        // curated the list and opted into repairing ordinary words against it.
        if !user_index.is_empty() {
            if let Some(&word) = user_index.get(&needle) {
                return word.to_owned(); // exact phonetic-key hit
            }
            let keys = user_index.keys().map(String::as_str);
            if let Some((hit, _)) = fuzzy_match::best_fuzzy_match(&needle, keys) {
                if let Some(&word) = user_index.get(hit) {
                    if word.chars().count() >= 4 {
                        return word.to_owned();
                    }
                }
            }
        }

        token.to_owned()
    })
}

/// Multi-word built-in targets, keyed by the phonetic key of the whole surface
/// with separators removed. Only targets that actually span more than one word
/// (contain a space or hyphen) are here; single words are handled per-token.
static MULTIWORD_BUILTINS: LazyLock<HashMap<String, &'static str>> = LazyLock::new(|| {
    let mut by_key: HashMap<String, Option<&'static str>> = HashMap::new();
    for &surface in ASR_TARGETS.values() {
        if !surface.contains([' ', '-', '\u{2011}', '–', '—']) {
            continue;
        }
        let key = fuzzy_match::canonical_key(surface);
        by_key
            .entry(key)
            .and_modify(|slot| {
                if *slot != Some(surface) {
                    *slot = None;
                }
            })
            .or_insert(Some(surface));
    }
    by_key.into_iter().filter_map(|(k, v)| v.map(|s| (k, s))).collect()
});

/// The most words any known multi-word target spans (window ceiling).
fn max_target_words(user_vocabulary: &[String]) -> usize {
    let builtin = MULTIWORD_BUILTINS.values().map(|s| word_count(s)).max().unwrap_or(0);
    let user = user_vocabulary
        .iter()
        .filter(|w| w.contains([' ', '-', '\u{2011}', '–', '—']))
        .map(|w| word_count(w))
        .max()
        .unwrap_or(0);
    builtin.max(user).max(1)
}

/// Counts sub-words in a surface form split on spaces and hyphens.
fn word_count(surface: &str) -> usize {
    surface.split([' ', '-', '\u{2011}', '–', '—']).filter(|p| !p.is_empty()).count()
}

/// Rejoins a multi-word target that a recognizer split across tokens.
///
/// Scans the Cyrillic tokens left to right; at each position it tries the
/// longest window first (down to two tokens), folds the window to one phonetic
/// key (separators are already dropped by [`canonical_key`], so a split reading
/// and its glued form share a key), and on a unique match against a multi-word
/// target replaces the whole span with the canonical surface. Non-matching text
/// — including single tokens — is emitted unchanged for the per-token pass.
fn join_multiword_targets(text: &str, user_vocabulary: &[String]) -> String {
    let max_words = max_target_words(user_vocabulary);
    if max_words < 2 {
        return text.to_owned();
    }
    // Per-call user multi-word index (space/hyphen entries only).
    let user_multi: HashMap<String, &str> = {
        let mut by_key: HashMap<String, Option<&str>> = HashMap::new();
        for w in user_vocabulary {
            if !w.contains([' ', '-', '\u{2011}', '–', '—']) {
                continue;
            }
            let key = fuzzy_match::canonical_key(w);
            by_key
                .entry(key)
                .and_modify(|slot| {
                    if *slot != Some(w.as_str()) {
                        *slot = None;
                    }
                })
                .or_insert(Some(w.as_str()));
        }
        by_key.into_iter().filter_map(|(k, v)| v.map(|w| (k, w))).collect()
    };

    // Tokenize into (start, end, is_word) spans over the original text so the
    // separators between tokens are preserved when nothing matches.
    let is_word_char =
        |c: char| is_uk(c) && !matches!(c, '\'' | '’' | '`' | '\u{02bc}' | '-' | '–' | '—');
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    // Word token boundaries (letters only; joiners split words here, since a
    // window join concerns whole spoken words).
    let mut words: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if is_word_char(chars[i].1) {
            let start = chars[i].0;
            while i < chars.len() && is_word_char(chars[i].1) {
                i += 1;
            }
            let end = if i < chars.len() { chars[i].0 } else { text.len() };
            words.push((start, end));
        } else {
            i += 1;
        }
    }

    let lookup = |key: &str| -> Option<&str> {
        MULTIWORD_BUILTINS.get(key).copied().or_else(|| user_multi.get(key).copied())
    };

    let mut out = String::with_capacity(text.len());
    let mut copied = 0; // byte offset copied into `out`
    let mut w = 0; // index into `words`
    while w < words.len() {
        let mut matched = None;
        let upper = (w + max_words).min(words.len());
        // Longest window first.
        for end in (w + 2..=upper).rev() {
            let mut joined = String::new();
            for &(s, e) in &words[w..end] {
                joined.push_str(&fuzzy_match::canonical_key(&text[s..e]));
            }
            if let Some(surface) = lookup(&joined) {
                matched = Some((end, surface));
                break;
            }
        }
        if let Some((end, surface)) = matched {
            let span_start = words[w].0;
            let span_end = words[end - 1].1;
            out.push_str(&text[copied..span_start]);
            out.push_str(surface);
            copied = span_end;
            w = end;
        } else {
            w += 1;
        }
    }
    if copied == 0 {
        return text.to_owned();
    }
    out.push_str(&text[copied..]);
    out
}
