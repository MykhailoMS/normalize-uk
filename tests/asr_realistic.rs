//! Realistic ASR-output scenarios.
//!
//! Each case is a whole phrase the way a Ukrainian speech recognizer would
//! actually hand it over — lost apostrophes and soft signs, confused vowels,
//! glued or split brand names, phonetically-spelled acronyms, surzhyk carry-over
//! — paired with what the normalizer should produce under `InputTolerance::Asr`.
//!
//! Run just these with:  cargo test --test asr_realistic

use normalize_uk::uktextnorm::{normalize_with, InputTolerance, NormalizeOptions};

fn asr() -> NormalizeOptions {
    NormalizeOptions { input_tolerance: InputTolerance::Asr, ..NormalizeOptions::default() }
}

fn asr_with(words: &[&str]) -> NormalizeOptions {
    NormalizeOptions {
        input_tolerance: InputTolerance::Asr,
        asr_vocabulary: words.iter().map(|w| (*w).to_owned()).collect(),
        ..NormalizeOptions::default()
    }
}

/// Asserts the normalized ASR output contains `needle`, printing both on
/// failure so a regression is easy to read.
fn assert_contains(input: &str, needle: &str, options: &NormalizeOptions) {
    let out = normalize_with(input, options);
    assert!(out.contains(needle), "input {input:?}\n  got: {out:?}\n  want substring: {needle:?}");
}

/// Asserts the phrase is returned unchanged (nothing was wrongly "repaired").
fn assert_unchanged(input: &str, options: &NormalizeOptions) {
    let out = normalize_with(input, options);
    assert_eq!(out, input, "phrase should be left alone but was rewritten");
}

// --- Brand names the recognizer garbled --------------------------------------

#[test]
fn garbled_brand_names_in_a_sentence() {
    // "ватсап" (о->а), "ютюб" (u->yu), "вайбэр" (surzhyk э)
    assert_contains("напиши мені у ватсап", "вотсап", &asr());
    assert_contains("подивись це відео на ютюб", "ютуб", &asr());
    assert_contains("додай мене у вайбэр", "вайбер", &asr());
}

#[test]
fn split_and_glued_spellings() {
    // A word not in the built-in lexicon (Wi-Fi) is repaired once the caller
    // supplies it — separators are folded so "вайфай" reaches "вай-фай".
    let options = asr_with(&["вай-фай"]);
    assert_contains("увімкни вайфай будь ласка", "вай-фай", &options);
}

// --- Acronyms: lowercased, and phonetically spelled --------------------------

#[test]
fn lowercased_acronym_is_expanded() {
    // ASR drops the caps: "пдв" -> ПДВ -> full expansion.
    assert_contains("нарахуйте пдв на суму", "додану вартість", &asr());
}

#[test]
fn phonetically_spelled_acronyms_are_expanded() {
    // Spelled as heard: пе-де-ве, ес-бе-у. (Expansion is capitalized at a
    // sentence-like position, so match a case-insensitive stem.)
    assert_contains("сплатив педеве вчора", "додану вартість", &asr());
    assert_contains("це справа есбеу", "безпеки", &asr());
}

// --- Lost apostrophes and soft signs -----------------------------------------

#[test]
fn a_domain_word_with_lost_apostrophe_is_repaired() {
    // "обєкт" (lost apostrophe) with the caller's word list present.
    let options = asr_with(&["об'єкт", "з'єднання"]);
    assert_contains("зафіксовано обєкт", "об'єкт", &options);
    assert_contains("розірвано зєднання", "з'єднання", &options);
}

// --- Surzhyk / vowel confusion inside a domain word --------------------------

#[test]
fn distorted_domain_terms_are_repaired_via_vocabulary() {
    let options = asr_with(&["автентифікація", "ідентифікатор", "верифікація"]);
    // і/и confusion, dropped letter, and another і/и confusion.
    assert_contains("потрібна автентіфікація", "автентифікація", &options);
    assert_contains("введіть ідентифікаор", "ідентифікатор", &options);
    assert_contains("пройдено верифікацію", "верифікаці", &options); // stem matches
}

// --- The pipeline still does its normal job on the repaired text -------------

#[test]
fn repaired_text_flows_through_the_rest_of_the_pipeline() {
    // Distorted acronym plus a number+unit in one breath: the acronym is
    // restored AND the measurement is read.
    let out = normalize_with("пдв дорівнює 5 кг", &asr());
    assert!(out.contains("додану вартість"), "acronym not expanded: {out}");
    assert!(out.contains("кілограм"), "unit not read: {out}");
}

// --- Safety: ordinary speech is never over-corrected -------------------------

#[test]
fn ordinary_dictated_sentences_are_left_intact() {
    // Real Ukrainian dictation with no lexicon words — must pass through as-is.
    for phrase in [
        "сьогодні я їхав на роботу дуже довго",
        "вона сказала що прийде трохи пізніше",
        "діти гралися у дворі цілий день",
        "ми обговорили всі важливі питання",
    ] {
        assert_unchanged(phrase, &asr());
    }
}

#[test]
fn a_distorted_ordinary_word_stays_untouched_without_a_vocabulary() {
    // No user vocabulary: an everyday distorted word is NOT force-matched to a
    // brand or acronym (that would corrupt prose).
    assert_unchanged("потрібна автентіфікація", &asr());
}

// --- Multi-word join / split ------------------------------------------------

#[test]
fn a_split_multiword_target_is_rejoined() {
    // A recognizer split "вай-фай" into two tokens; the window pass rejoins it.
    let options = asr_with(&["вай-фай"]);
    assert_contains("увімкни вай фай будь ласка", "вай-фай", &options);
}

#[test]
fn a_glued_multiword_target_is_split_back() {
    // The glued form folds to the same phonetic key and is restored.
    let options = asr_with(&["вай-фай"]);
    assert_contains("увімкни вайфай будь ласка", "вай-фай", &options);
}

#[test]
fn a_distorted_split_multiword_target_is_rejoined() {
    // Split AND vowel-distorted ("вай" + "фай" with і/и noise) still rejoins.
    let options = asr_with(&["дата-центр"]);
    assert_contains("переніс усе в дата центр", "дата-центр", &options);
    assert_contains("переніс усе в датацентр", "дата-центр", &options);
}

#[test]
fn multiword_join_leaves_ordinary_two_word_prose_alone() {
    let options = asr_with(&["вай-фай", "дата-центр"]);
    let sentence = "він пішов у магазин по хліб";
    assert_unchanged(sentence, &options);
}

// --- Akannya (о/а) now resolves on the exact phonetic key --------------------

#[test]
fn akannya_is_repaired_without_touching_prose() {
    assert_contains("переказ через монабанк", "монобанк", &asr());
    assert_contains("напиши у ватсап", "вотсап", &asr());
    // A real word one consonant away from a short target is NOT changed.
    assert_unchanged("звичайний текст", &asr());
}

#[test]
fn a_full_noisy_dictation() {
    // monobank distorted (о/а), ПДВ spelled phonetically, plus a caller-supplied
    // Wi-Fi. A realistic voice command with several distortions at once.
    let input = "перекажи гроші через монабанк підключи вайфай і сплати педеве";
    let out = normalize_with(input, &asr_with(&["вай-фай"]));
    assert!(out.contains("монобанк"), "monobank not fixed: {out}");
    assert!(out.contains("вай-фай"), "wifi not fixed: {out}");
    assert!(out.contains("додану вартість"), "ПДВ not expanded: {out}");
    // Ordinary words survive untouched.
    assert!(out.contains("перекажи гроші через"), "prose damaged: {out}");
}
