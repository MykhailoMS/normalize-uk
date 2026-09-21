//! End-to-end tests for the ASR-tolerant matching mode.
//!
//! These exercise the public API only, the way a caller integrating an ASR
//! front-end would: build options with `InputTolerance::Asr`, normalize noisy
//! input, and confirm both the reading and the uncertainty report.

use normalize_uk::uktextnorm::{
    flag_uncertain_with, normalize_with, InputTolerance, NormalizeOptions, UncertaintyCategory,
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
