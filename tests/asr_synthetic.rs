//! Synthetic ASR-distortion tests.
//!
//! Rather than hand-write each distorted spelling, this suite *generates* the
//! kinds of errors a Ukrainian speech recognizer makes — lost apostrophes and
//! soft signs, front-vowel confusion (і/и, е/є), о/а akannya, dropped iotation,
//! ґ→г, doublings, and dropped separators — applies them deterministically to
//! known lexicon targets, and asserts that `InputTolerance::Asr` recovers the
//! canonical form while `Strict` does not.
//!
//! The distorter is seeded and dependency-free, so runs are reproducible.
//!
//! Run just these with:  cargo test --test asr_synthetic

use normalize_uk::uktextnorm::{normalize_with, InputTolerance, NormalizeOptions};

// --- A tiny deterministic PRNG (xorshift) so runs are reproducible -----------

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.max(1))
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    /// True with probability `1/n`.
    fn chance(&mut self, n: u64) -> bool {
        self.next() % n == 0
    }
}

/// Applies a spread of realistic ASR distortions to one word.
///
/// Every rule mirrors a fold in the crate's phonetic canonical key, so a
/// distorted form should collapse to the same key as the original and be
/// repaired. Distortions fire probabilistically; the seed makes it repeatable.
fn distort(word: &str, rng: &mut Rng) -> String {
    let mut out = String::new();
    let mut prev = '\0';
    for ch in word.chars() {
        // Drop an apostrophe or soft sign (ASR rarely writes them).
        if matches!(ch, '\'' | '’' | 'ь') && rng.chance(2) {
            continue;
        }
        let mut c = ch;
        // Front-vowel confusions.
        if c == 'і' && rng.chance(2) {
            c = 'и';
        } else if c == 'ї' && rng.chance(2) {
            c = 'і';
        } else if c == 'є' && rng.chance(3) {
            c = 'е';
        }
        // о/а akannya and iotation loss (я→а) both collapse to 'а'.
        else if (c == 'о' || c == 'я') && rng.chance(3) {
            c = 'а';
        }
        // Iotation loss on ю.
        else if c == 'ю' && rng.chance(3) {
            c = 'у';
        }
        // ґ merges to г.
        else if c == 'ґ' {
            c = 'г';
        }
        // Occasionally double a consonant (mis-heard gemination).
        if c == prev && rng.chance(4) {
            out.push(c);
        }
        out.push(c);
        prev = c;
    }
    out
}

/// The canonical lexicon targets we can prove recovery for. Distorting an
/// arbitrary open-vocabulary word could not be recovered without a dictionary,
/// which is by design — so the synthetic corpus is drawn from closed targets.
const TARGETS: &[&str] = &[
    "вотсап",
    "ютуб",
    "вайбер",
    "телеграм",
    "інстаграм",
    "фейсбук",
    "спотіфай",
    "монобанк",
    "приватбанк",
    "київстар",
    "епіцентр",
    "розетка",
];

fn asr() -> NormalizeOptions {
    NormalizeOptions { input_tolerance: InputTolerance::Asr, ..NormalizeOptions::default() }
}

#[test]
fn synthetic_distortions_are_recovered() {
    let mut rng = Rng::new(0xA5A5_1234);
    let options = asr();
    let mut checked = 0;
    let mut recovered = 0;
    let mut failures = Vec::new();

    for &target in TARGETS {
        // Several independent distortions per target.
        for _ in 0..40 {
            let noisy = distort(target, &mut rng);
            if noisy == target {
                continue; // no distortion fired this draw
            }
            checked += 1;
            let out = normalize_with(&noisy, &options);
            if out == target {
                recovered += 1;
            } else {
                failures.push(format!("{noisy:?} -> {out:?} (want {target:?})"));
            }
        }
    }

    let rate = recovered as f64 / checked.max(1) as f64;
    // The phonetic key + guarded 1-edit repair should recover the large
    // majority of single-distortion tokens. We assert a firm floor rather than
    // 100%, because a draw can stack several edits past the budget.
    assert!(
        rate >= 0.80,
        "recovered {recovered}/{checked} = {rate:.2}; first failures:\n{}",
        failures.iter().take(10).cloned().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn strict_mode_recovers_almost_none_of_the_distortions() {
    // Control: without ASR tolerance the same distortions are NOT repaired,
    // proving the recovery above is the feature and not the base pipeline.
    let mut rng = Rng::new(0xA5A5_1234);
    let strict = NormalizeOptions::default();
    let mut checked = 0;
    let mut unchanged_from_target = 0;

    for &target in TARGETS {
        for _ in 0..40 {
            let noisy = distort(target, &mut rng);
            if noisy == target {
                continue;
            }
            checked += 1;
            if normalize_with(&noisy, &strict) != target {
                unchanged_from_target += 1;
            }
        }
    }
    // Strict must leave essentially all distortions unrepaired.
    let miss_rate = unchanged_from_target as f64 / checked.max(1) as f64;
    assert!(miss_rate >= 0.95, "strict unexpectedly repaired some: miss_rate={miss_rate:.2}");
}

#[test]
fn synthetic_distortions_never_corrupt_clean_prose() {
    // A corpus of ordinary Ukrainian words (no lexicon targets): distort them
    // and confirm ASR mode never "repairs" one into a target — prose is safe.
    const PROSE: &[&str] = &[
        "сьогодні",
        "робота",
        "дорога",
        "питання",
        "дитина",
        "будинок",
        "погода",
        "місто",
        "вулиця",
        "розмова",
        "коробка",
        "телефон",
        "документи",
        "працювати",
    ];
    let mut rng = Rng::new(0x1357_9BDF);
    let options = asr();
    for &word in PROSE {
        for _ in 0..30 {
            let noisy = distort(word, &mut rng);
            let out = normalize_with(&noisy, &options);
            // ASR mode must not turn a distorted ordinary word into a lexicon
            // target; it either leaves it or (harmlessly) equals the noisy form.
            assert!(
                !TARGETS.contains(&out.as_str()),
                "prose {noisy:?} was wrongly repaired into target {out:?}"
            );
        }
    }
}

#[test]
fn a_sentence_of_distorted_targets_amid_prose() {
    // Targets embedded in a real sentence, each distorted, prose untouched.
    let options = asr();
    let out = normalize_with("скинь у вайбэр посилання на ютюб і монабанк", &options);
    assert!(out.contains("вайбер"), "viber: {out}");
    assert!(out.contains("ютуб"), "youtube: {out}");
    assert!(out.contains("монобанк"), "monobank: {out}");
    assert!(out.contains("скинь у") && out.contains("посилання на"), "prose damaged: {out}");
}
