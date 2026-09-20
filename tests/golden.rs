//! Golden-file tests, driven by the TSV corpora under `tests/data/`.

use normalize_uk::uktextnorm::{normalize, normalize_preset, NormalizePreset};

/// Yields the `(input, expected)` rows of a two-column golden file.
fn rows(source: &str) -> impl Iterator<Item = (usize, &str, &str)> {
    source.lines().enumerate().filter_map(|(index, raw)| {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let (input, expected) = line.split_once('\t').expect("expected two columns");
        Some((index + 1, input, expected))
    })
}

#[track_caller]
fn check_golden(name: &str, source: &str) {
    let mut failures = Vec::new();
    for (row, input, expected) in rows(source) {
        let actual = normalize(input);
        if actual != expected {
            failures.push(format!(
                "{name} row {row}\n  input:    {input}\n  expected: {expected}\n  actual:   {actual}"
            ));
        }
    }
    assert!(failures.is_empty(), "{} failure(s)\n{}", failures.len(), failures.join("\n"));
}

macro_rules! golden {
    ($name:ident, $file:literal) => {
        #[test]
        fn $name() {
            check_golden($file, include_str!(concat!("data/", $file)));
        }
    };
}

golden!(general, "uktextnorm_golden.tsv");
golden!(domain, "uktextnorm_domain_golden.tsv");
golden!(coverage, "uktextnorm_coverage_golden.tsv");
golden!(morphology, "uktextnorm_morphology_golden.tsv");
golden!(robust, "uktextnorm_robust_golden.tsv");

/// The sentence corpus is three columns and uses the TTS preset, and every row
/// must also be idempotent.
#[test]
fn sentences() {
    let source = include_str!("data/uktextnorm_sentence_golden.tsv");
    let mut failures = Vec::new();
    let mut categories = std::collections::HashMap::new();
    for (index, raw) in source.lines().enumerate() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let row = index + 1;
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(fields.len(), 3, "row {row}: expected category, input and output");
        let (category, input, expected) = (fields[0], fields[1], fields[2]);
        *categories.entry(category).or_insert(0usize) += 1;
        let actual = normalize_preset(input, NormalizePreset::TtsFriendly);
        if actual != expected {
            failures.push(format!(
                "{category} row {row}\n  input:    {input}\n  expected: {expected}\n  actual:   {actual}"
            ));
            continue;
        }
        let repeated = normalize_preset(&actual, NormalizePreset::TtsFriendly);
        if repeated != actual {
            failures.push(format!(
                "{category} row {row} is not idempotent\n  first:  {actual}\n  second: {repeated}"
            ));
        }
    }
    for (category, count) in &categories {
        assert!(*count >= 2, "category {category} has only {count} case(s)");
    }
    assert!(categories.len() >= 20, "only {} categories covered", categories.len());
    assert!(failures.is_empty(), "{} failure(s)\n{}", failures.len(), failures.join("\n"));
}
