//! ASR-tolerant matching for closed lexicons.
//!
//! Speech recognition rarely hands the normalizer a clean token. The same word
//! arrives with a missing apostrophe (`дев'ятнадцятого` -> `девятнадцятого`),
//! glued or split (`ю ес бі` -> `юесбі`, `вайфай` -> `вай фай`), or in a
//! surzhyk / phonetic variant (`тищя` -> `тисяча`). The exact-key lookups the
//! rest of the crate relies on then miss, and the token passes through raw.
//!
//! This module adds a *fallback*, not a replacement. It never runs on the hot
//! path for clean text: a caller reaches for it only after an exact lookup has
//! already missed. Two cheap, deterministic stages resolve the miss against a
//! *closed* set of known keys — never against free text — so the search space
//! is hundreds of entries, not millions, and the behaviour cannot drift:
//!
//! 1. [`canonical_key`] folds separators and a few ASR-confusable spellings so
//!    that `вай-фай`, `вай фай` and `вайфай` collapse to one key. A lexicon
//!    indexed by the same canonical key then matches directly.
//! 2. [`best_fuzzy_match`] takes the closest entry within a bounded
//!    Damerau–Levenshtein distance (one edit for short keys, two for long
//!    ones), rejecting ties so an ambiguous token is left untouched rather than
//!    guessed.
//!
//! Both stages are pure functions over `&str`. Wiring them into the pipeline —
//! building the canonical index and reporting a fallback hit through
//! `flag_uncertain` — is deliberately left to the caller.

use super::text::lower_cp;

/// The maximum edit distance allowed for a fuzzy match, by key length.
///
/// Short keys tolerate a single edit; longer ones tolerate two. A word that
/// needs more than this to reach any lexicon entry is not a confident match and
/// is left alone.
fn distance_budget(key_len: usize) -> usize {
    if key_len <= 4 {
        1
    } else {
        2
    }
}

/// Folds a token to a canonical phonetic key that ignores the spellings ASR
/// varies for Ukrainian speech.
///
/// The fold is built from the distortions a Ukrainian recognizer actually
/// produces, so tokens that *sound the same* collapse to one key. It is applied
/// only against closed, foreign-shaped target sets (brand readings, acronym
/// spellings), where merging near-homophones is safe — it is never a general
/// speller over open prose.
///
/// What it folds, and why each is an ASR reality:
/// - separators (space / hyphen / apostrophe) — added or dropped at random;
/// - front-vowel confusions `і`/`ї` → `и`, `є` → `е` — the most common
///   Ukrainian ASR error class (unstressed vowel reduction);
/// - iotated back vowels `я`→`а`, `ю`→`у` — the glide is often lost;
/// - `ґ`→`г` — routinely merged;
/// - Russian/surzhyk carry-over `ы`→`и`, `э`→`е`, `ё`→`о`, `ъ`→dropped;
/// - the soft sign `ь` — a secondary articulation ASR rarely writes;
/// - immediate doublings — collapsed (`нн`, `сс` mis-heard as single).
///
/// ```ignore
/// assert_eq!(canonical_key("вай-фай"), canonical_key("вай фай"));
/// assert_eq!(canonical_key("спотифай"), canonical_key("спотіфай"));
/// assert_eq!(canonical_key("дев'ятнадцятого"), canonical_key("девятнадцятого"));
/// ```
#[must_use]
pub(crate) fn canonical_key(token: &str) -> String {
    let mut out = String::with_capacity(token.len());
    let mut previous = '\0';
    for cp in token.chars() {
        // Separators carry no sound: a hyphen, space or apostrophe between the
        // same phonemes is exactly what ASR adds or drops at random.
        if matches!(cp, ' ' | '\t' | '-' | '‑' | '–' | '—' | '\'' | '’' | '`' | '\u{02bc}')
        {
            continue;
        }
        let cp = lower_cp(cp);
        // Fold near-homophones onto one representative. Every mapping is an ASR
        // confusion for Ukrainian; because the fallback only compares against a
        // closed foreign-shaped set, none of these merges two real targets.
        let folded = match cp {
            // Front vowels: the dominant unstressed-reduction confusion.
            'і' | 'ї' => 'и',
            'є' => 'е',
            // Iotated back vowels: the glide is frequently dropped.
            'я' => 'а',
            'ю' => 'у',
            // о/а akannya — a very common Ukrainian ASR confusion in unstressed
            // position (`монобанк`->`монабанк`, `вотсап`->`ватсап`). Folded here
            // so those resolve on the exact phonetic key; against the closed
            // target set this does not merge two real targets (collisions are
            // dropped when the index is built).
            'о' => 'а',
            // Routinely merged consonant and its Russian twin.
            'ґ' => 'г',
            // Surzhyk / Russian carry-over from the input side.
            'ы' => 'и',
            'э' => 'е',
            'ё' => 'о',
            // The soft sign and hard sign carry no vowel; drop them.
            'ь' | 'ъ' => '\0',
            other => other,
        };
        if folded == '\0' {
            continue; // dropped letter (soft / hard sign)
        }
        // Collapse an immediate doubling (`нн`, `сс` mis-heard as a single).
        if folded == previous {
            continue;
        }
        previous = folded;
        out.push(folded);
    }
    out
}

/// The Damerau–Levenshtein distance between `a` and `b`, capped at `max`.
///
/// Returns `max + 1` as soon as the true distance is known to exceed `max`, so
/// the common "too far" case stops early instead of filling the whole table.
/// Operates on `char`s, so a multi-byte Cyrillic letter counts as one edit.
fn bounded_damerau_levenshtein(a: &[char], b: &[char], max: usize) -> usize {
    let (n, m) = (a.len(), b.len());
    if n.abs_diff(m) > max {
        return max + 1;
    }
    // Two-row DP is not enough for transpositions, so keep three rows.
    let width = m + 1;
    let mut prev_prev = vec![0usize; width];
    let mut prev = (0..=m).collect::<Vec<_>>();
    let mut curr = vec![0usize; width];
    for i in 1..=n {
        curr[0] = i;
        let mut row_min = curr[0];
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(prev_prev[j - 2] + 1);
            }
            curr[j] = best;
            row_min = row_min.min(best);
        }
        // Every remaining row can only grow, so bail once a whole row exceeds
        // the budget.
        if row_min > max {
            return max + 1;
        }
        std::mem::swap(&mut prev_prev, &mut prev);
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[m]
}

/// The single closest candidate to `token` within the length-scaled budget.
///
/// `candidates` is a closed set of canonical keys — a lexicon, not free text.
/// Returns the best key and its distance, or `None` when nothing is close
/// enough *or* two candidates tie for closest. A tie is deliberately a
/// non-match: an ambiguous token must not be silently resolved to one arbitrary
/// reading.
///
/// ```ignore
/// let keys = ["spotify", "spot", "shopify"];
/// let (hit, dist) = best_fuzzy_match("spotifay", keys.iter().copied()).unwrap();
/// assert_eq!(hit, "spotify");
/// assert_eq!(dist, 1);
/// ```
#[must_use]
pub(crate) fn best_fuzzy_match<'a, I>(token: &str, candidates: I) -> Option<(&'a str, usize)>
where
    I: IntoIterator<Item = &'a str>,
{
    let needle: Vec<char> = token.chars().collect();
    let budget = distance_budget(needle.len());
    best_fuzzy_match_within(token, candidates, budget)
}

/// Like [`best_fuzzy_match`] but with an explicit maximum edit distance, for
/// callers that want a stricter budget than the length-scaled default (e.g. a
/// single edit against a lexicon that sits in open prose).
#[must_use]
pub(crate) fn best_fuzzy_match_within<'a, I>(
    token: &str,
    candidates: I,
    max: usize,
) -> Option<(&'a str, usize)>
where
    I: IntoIterator<Item = &'a str>,
{
    let needle: Vec<char> = token.chars().collect();
    let budget = max;
    let mut best: Option<(&str, usize)> = None;
    let mut tied = false;
    for candidate in candidates {
        let hay: Vec<char> = candidate.chars().collect();
        let dist = bounded_damerau_levenshtein(&needle, &hay, budget);
        if dist > budget {
            continue;
        }
        match best {
            Some((_, best_dist)) if dist < best_dist => {
                best = Some((candidate, dist));
                tied = false;
            }
            Some((_, best_dist)) if dist == best_dist => tied = true,
            Some(_) => {}
            None => best = Some((candidate, dist)),
        }
        // An exact match cannot be beaten; stop once one is found.
        if dist == 0 {
            return Some((candidate, 0));
        }
    }
    match best {
        Some((_, _)) if tied => None,
        other => other,
    }
}

/// Resolves `token` against a closed set of `(key, reading)` pairs, tolerating
/// ASR distortion. Returns the reading and how it was reached, or `None`.
///
/// The three stages run in order and each only fires when the previous misses,
/// so clean input resolves on the first (exact) stage and never pays for the
/// rest:
///
/// 1. **Exact** — `token`, lowercased, equals a key verbatim.
/// 2. **Canonical** — [`canonical_key`] of the token equals the canonical key
///    of exactly one entry (separators and confusable spellings folded away).
/// 3. **Fuzzy** — the token is within a bounded edit distance of exactly one
///    canonical key.
///
/// The returned [`MatchKind`] lets the caller report stages 2 and 3 through
/// `flag_uncertain`, so an approximate reading is always visible.
#[must_use]
pub(crate) fn resolve<'a>(
    token: &str,
    entries: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Option<(&'a str, MatchKind)> {
    let lowered = super::text::lower_text(token);
    let needle_canon = canonical_key(token);

    // One pass over the closed set builds what each stage needs.
    let mut exact: Option<&str> = None;
    let mut canon_hits: Vec<(String, &str)> = Vec::new(); // (canonical_key, reading)
    for (key, reading) in entries {
        if key == lowered {
            exact = Some(reading);
            break;
        }
        canon_hits.push((canonical_key(key), reading));
    }
    if let Some(reading) = exact {
        return Some((reading, MatchKind::Exact));
    }

    // Stage 2: unique canonical-key equality.
    let mut canonical: Option<&str> = None;
    let mut canonical_tied = false;
    for (canon, reading) in &canon_hits {
        if *canon == needle_canon {
            if canonical.is_some() {
                canonical_tied = true;
            } else {
                canonical = Some(reading);
            }
        }
    }
    if let Some(reading) = canonical {
        if !canonical_tied {
            return Some((reading, MatchKind::Canonical));
        }
    }

    // Stage 3: closest canonical key within the edit-distance budget.
    let canon_keys = canon_hits.iter().map(|(canon, _)| canon.as_str());
    let (hit_key, _) = best_fuzzy_match(&needle_canon, canon_keys)?;
    // Map the winning canonical key back to its reading (first wins, matching
    // the lexicon's own brands-before-english precedence).
    canon_hits
        .iter()
        .find(|(canon, _)| canon == hit_key)
        .map(|(_, reading)| (*reading, MatchKind::Fuzzy))
}

/// How a [`resolve`] hit was reached, for uncertainty reporting.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum MatchKind {
    /// The token matched a key verbatim.
    Exact,
    /// The token matched after folding separators / confusable spellings.
    Canonical,
    /// The token matched the closest key within a bounded edit distance.
    Fuzzy,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separators_do_not_affect_the_key() {
        let base = canonical_key("вай фай");
        assert_eq!(canonical_key("вай-фай"), base);
        assert_eq!(canonical_key("вайфай"), base);
        assert_eq!(canonical_key("вай—фай"), base);
    }

    #[test]
    fn apostrophe_variants_collapse() {
        let base = canonical_key("дев'ятнадцятого");
        assert_eq!(canonical_key("девятнадцятого"), base);
        assert_eq!(canonical_key("дев’ятнадцятого"), base);
        assert_eq!(canonical_key("дев\u{02bc}ятнадцятого"), base);
    }

    #[test]
    fn spelled_letters_glue_and_split_to_one_key() {
        let base = canonical_key("ю ес бі");
        assert_eq!(canonical_key("юесбі"), base);
        assert_eq!(canonical_key("ю-ес-бі"), base);
    }

    #[test]
    fn distinct_words_keep_distinct_keys() {
        assert_ne!(canonical_key("тисяча"), canonical_key("тиждень"));
        assert_ne!(canonical_key("google"), canonical_key("doodle"));
    }

    #[test]
    fn phonetic_key_folds_front_vowel_confusions() {
        // і / ї / и collapse; є / е collapse — the dominant ASR vowel errors.
        assert_eq!(canonical_key("спотіфай"), canonical_key("спотифай"));
        assert_eq!(canonical_key("вайбер"), canonical_key("вайбэр"));
        assert_eq!(canonical_key("їжак"), canonical_key("ижак"));
    }

    #[test]
    fn phonetic_key_folds_iotation_and_soft_sign() {
        // я -> а, ю -> у, soft sign dropped.
        assert_eq!(canonical_key("пятьсот"), canonical_key("п'ятсот"));
        assert_eq!(canonical_key("сьогодні"), canonical_key("согодни"));
    }

    #[test]
    fn phonetic_key_folds_g_variants_and_doublings() {
        assert_eq!(canonical_key("ґуґл"), canonical_key("гугл"));
        assert_eq!(canonical_key("ссавці"), canonical_key("савці"));
    }

    #[test]
    fn phonetic_key_folds_akannya() {
        // о and а share a key (unstressed о/а confusion).
        assert_eq!(canonical_key("монобанк"), canonical_key("монабанк"));
        assert_eq!(canonical_key("вотсап"), canonical_key("ватсап"));
    }

    #[test]
    fn phonetic_key_still_separates_genuinely_different_words() {
        // Folding must not turn unrelated words into one key.
        assert_ne!(canonical_key("телеграм"), canonical_key("телефон"));
        assert_ne!(canonical_key("гугл"), canonical_key("дудл"));
    }

    #[test]
    fn fuzzy_matches_a_single_edit() {
        let keys = ["spotify", "spot", "shopify"];
        let (hit, dist) = best_fuzzy_match("spotifay", keys.iter().copied()).unwrap();
        assert_eq!(hit, "spotify");
        assert_eq!(dist, 1);
    }

    #[test]
    fn fuzzy_matches_a_transposition_as_one_edit() {
        let keys = ["telegram"];
        let (hit, dist) = best_fuzzy_match("teelgram", keys.iter().copied()).unwrap();
        assert_eq!(hit, "telegram");
        assert_eq!(dist, 1);
    }

    #[test]
    fn fuzzy_rejects_when_too_far() {
        let keys = ["spotify"];
        assert!(best_fuzzy_match("banana", keys.iter().copied()).is_none());
    }

    #[test]
    fn fuzzy_rejects_a_tie() {
        // "cat" is one edit from both "car" and "bat": ambiguous, so no match.
        let keys = ["car", "bat"];
        assert!(best_fuzzy_match("cat", keys.iter().copied()).is_none());
    }

    #[test]
    fn short_keys_tolerate_only_one_edit() {
        // Two edits away from a 4-char key exceeds the budget of 1.
        let keys = ["node"];
        assert!(best_fuzzy_match("nada", keys.iter().copied()).is_none());
    }

    #[test]
    fn long_keys_tolerate_two_edits() {
        let keys = ["kubernetes"];
        let (hit, dist) = best_fuzzy_match("kubernets", keys.iter().copied()).unwrap();
        assert_eq!(hit, "kubernetes");
        assert!(dist <= 2);
    }

    #[test]
    fn cyrillic_edits_count_by_character_not_byte() {
        // One dropped Cyrillic letter is one edit, though it is two bytes:
        // "тисча" -> "тисяча" is a single insertion.
        let keys = ["тисяча"];
        let (hit, dist) = best_fuzzy_match("тисча", keys.iter().copied()).unwrap();
        assert_eq!(hit, "тисяча");
        assert_eq!(dist, 1);
    }

    fn brands() -> Vec<(&'static str, &'static str)> {
        vec![("whatsapp", "вотсап"), ("wifi", "вай-фай"), ("spotify", "спотіфай")]
    }

    #[test]
    fn resolve_exact_hit() {
        let (reading, kind) = resolve("whatsapp", brands()).unwrap();
        assert_eq!(reading, "вотсап");
        assert_eq!(kind, MatchKind::Exact);
    }

    #[test]
    fn resolve_canonical_hit_ignores_separators() {
        let (reading, kind) = resolve("wi-fi", brands()).unwrap();
        assert_eq!(reading, "вай-фай");
        assert_eq!(kind, MatchKind::Canonical);
    }

    #[test]
    fn resolve_fuzzy_hit_on_a_typo() {
        let (reading, kind) = resolve("spotifay", brands()).unwrap();
        assert_eq!(reading, "спотіфай");
        assert_eq!(kind, MatchKind::Fuzzy);
    }

    #[test]
    fn resolve_leaves_an_unrelated_token_alone() {
        assert!(resolve("banana", brands()).is_none());
    }
}
