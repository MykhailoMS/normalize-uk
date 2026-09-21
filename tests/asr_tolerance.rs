//! End-to-end tests for the ASR-tolerant matching mode.
//!
//! These exercise the public API only, the way a caller integrating an ASR
//! front-end would: build options with `InputTolerance::Asr`, normalize noisy
//! input, and confirm both the reading and the uncertainty report.

use normalize_uk::uktextnorm::{
    flag_uncertain_with, normalize_with, parse_asr_vocabulary, InputTolerance, NormalizeOptions,
    UncertaintyCategory,
};

fn asr_options() -> NormalizeOptions {
    NormalizeOptions { input_tolerance: InputTolerance::Asr, ..NormalizeOptions::default() }
}

#[test]
fn strict_mode_is_unchanged_by_default() {
    // Under strict tolerance the ASR fallback never runs, so a typo is not
    // resolved to a reading; it is only transliterated like any unknown Latin
    // word (the crate's existing default behaviour).
    let strict = NormalizeOptions::default();
    assert_eq!(strict.input_tolerance, InputTolerance::Strict);
    let out = normalize_with("spotifay", &strict);
    assert_ne!(out, "спотіфай"); // did NOT reach the "spotify" reading
}

#[test]
fn asr_mode_resolves_a_latin_typo_to_its_reading() {
    // "spotifay" is one edit from the "spotify" lexicon entry.
    let out = normalize_with("spotifay", &asr_options());
    assert_eq!(out, "спотіфай");
}

#[test]
fn asr_mode_leaves_a_clean_known_word_alone() {
    // Exact hits still resolve on the fast path, identically to strict mode.
    assert_eq!(normalize_with("spotify", &asr_options()), "спотіфай");
}

#[test]
fn asr_mode_does_not_touch_an_unrelated_word() {
    // A word far from every lexicon entry is not force-matched.
    let out = normalize_with("bananamobile", &asr_options());
    assert!(out.contains("bananamobile") || out.chars().any(char::is_alphabetic));
}

#[test]
fn approximate_matches_are_reported() {
    // The report must surface the approximate reading so it is never silent.
    let spans = flag_uncertain_with("spotifay", &asr_options());
    assert!(
        spans.iter().any(|s| s.category == UncertaintyCategory::ApproximateMatch),
        "expected an ApproximateMatch span, got: {spans:?}"
    );
}

#[test]
fn strict_mode_reports_no_approximate_matches() {
    let spans = flag_uncertain_with("spotifay", &NormalizeOptions::default());
    assert!(spans.iter().all(|s| s.category != UncertaintyCategory::ApproximateMatch));
}

// --- Second pass: distorted Cyrillic readings -------------------------------

#[test]
fn asr_mode_fixes_a_distorted_cyrillic_reading() {
    // "ватсап" is a one-edit distortion of the canonical reading "вотсап"
    // (whatsapp). The input is Cyrillic, so `normalize_english` never sees it —
    // only the Cyrillic pass can fix it.
    let out = normalize_with("ватсап", &asr_options());
    assert_eq!(out, "вотсап");
}

#[test]
fn asr_mode_fixes_a_cyrillic_vowel_confusion() {
    // "спотифай" -> "спотіфай" (и/і confusion), one edit.
    assert_eq!(normalize_with("спотифай", &asr_options()), "спотіфай");
}

#[test]
fn asr_mode_leaves_a_canonical_cyrillic_reading_untouched() {
    assert_eq!(normalize_with("вотсап", &asr_options()), "вотсап");
}

#[test]
fn strict_mode_does_not_touch_a_distorted_cyrillic_reading() {
    // Without ASR tolerance the distorted reading passes through unchanged.
    assert_eq!(normalize_with("ватсап", &NormalizeOptions::default()), "ватсап");
}

#[test]
fn asr_mode_leaves_ordinary_ukrainian_prose_alone() {
    // Everyday Ukrainian words must not be dragged onto a foreign reading.
    let sentence = "сьогодні вранці я пив каву";
    assert_eq!(normalize_with(sentence, &asr_options()), sentence);
}

// --- Extended targets: acronyms ---------------------------------------------

#[test]
fn asr_mode_restores_a_lowercased_acronym() {
    // ASR emits the ПДВ initialism glued and lowercased; the exact acronym rule
    // (which requires uppercase) misses it, so canonicalize_asr restores "ПДВ"
    // and the downstream acronym pass then expands it.
    let out = normalize_with("сума пдв велика", &asr_options());
    assert!(out.contains("додану вартість"), "expected the ПДВ expansion, got: {out}");
}

#[test]
fn strict_mode_leaves_a_lowercased_acronym_untouched() {
    let out = normalize_with("сума пдв велика", &NormalizeOptions::default());
    assert!(out.contains("пдв"));
    assert!(!out.contains("додану вартість"));
}

#[test]
fn asr_mode_does_not_invent_an_acronym_from_prose() {
    // A common word must not be pulled onto an acronym key.
    let sentence = "вони пили каву разом";
    assert_eq!(normalize_with(sentence, &asr_options()), sentence);
}

#[test]
fn asr_mode_restores_a_phonetically_spelled_acronym() {
    // ASR writes the ПДВ initialism as it sounds: пе-де-ве -> "педеве".
    let out = normalize_with("нарахували педеве", &asr_options());
    assert!(
        out.contains("додану вартість"),
        "expected the ПДВ expansion from a phonetic spelling, got: {out}"
    );
}

#[test]
fn strict_mode_leaves_a_phonetic_acronym_untouched() {
    let out = normalize_with("нарахували педеве", &NormalizeOptions::default());
    assert!(out.contains("педеве"));
    assert!(!out.contains("додану вартість"));
}

// --- Universal extension point: user vocabulary -----------------------------

fn options_with_vocab(words: &[&str]) -> NormalizeOptions {
    NormalizeOptions {
        input_tolerance: InputTolerance::Asr,
        asr_vocabulary: words.iter().map(|w| (*w).to_owned()).collect(),
        ..NormalizeOptions::default()
    }
}

#[test]
fn user_vocabulary_repairs_a_distorted_domain_word() {
    // A caller's own Ukrainian word list catches ordinary words the built-in
    // closed sets never would. "автентіфікація" -> "автентифікація" (і/и fold).
    let options = options_with_vocab(&["автентифікація"]);
    assert_eq!(normalize_with("автентіфікація", &options), "автентифікація");
}

#[test]
fn user_vocabulary_repairs_via_bounded_edit_distance() {
    // "ідентифікатор" mis-heard with a dropped letter, one edit away.
    let options = options_with_vocab(&["ідентифікатор"]);
    assert_eq!(normalize_with("ідентифікаор", &options), "ідентифікатор");
}

#[test]
fn user_vocabulary_leaves_a_far_word_alone() {
    let options = options_with_vocab(&["ідентифікатор"]);
    assert_eq!(normalize_with("будинок", &options), "будинок");
}

#[test]
fn empty_user_vocabulary_changes_nothing_beyond_builtins() {
    // With no user words, an ordinary distorted word is left as-is.
    assert_eq!(normalize_with("автентіфікація", &asr_options()), "автентіфікація");
}

#[test]
fn parse_asr_vocabulary_reads_one_column() {
    let words = parse_asr_vocabulary("word\nавтентифікація\nідентифікатор\n").unwrap();
    assert_eq!(words, vec!["автентифікація".to_owned(), "ідентифікатор".to_owned()]);
}

#[test]
fn parse_asr_vocabulary_rejects_a_missing_header() {
    assert!(parse_asr_vocabulary("автентифікація\n").is_err());
}
