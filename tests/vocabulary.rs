//! Loading and applying user-supplied word readings.

use normalize_uk::uktextnorm::{
    load_vocabulary_tsv, normalize, normalize_with, parse_vocabulary, NormalizeOptions,
    VocabularyError,
};

const FIXTURE: &str = "tests/data/custom_vocabulary.tsv";

#[test]
fn loads_a_vocabulary_file() {
    let words = load_vocabulary_tsv(FIXTURE).expect("fixture should load");
    // Keys are lowercased so lookups ignore case.
    assert_eq!(words.get("google").map(String::as_str), Some("гуголь"));
    assert_eq!(words.get("acme").map(String::as_str), Some("акме"));
    assert_eq!(words.len(), 2);
}

#[test]
fn vocabulary_overrides_the_built_in_reading() {
    let options = NormalizeOptions {
        vocabulary: load_vocabulary_tsv(FIXTURE).expect("fixture should load"),
        ..NormalizeOptions::default()
    };
    assert_eq!(normalize_with("Google і Acme", &options), "гуголь і акме");
    // Without the vocabulary, the built-in brand reading applies instead.
    assert_ne!(normalize("Google і Acme"), "гуголь і акме");
}

#[test]
fn rejects_malformed_files() {
    let cases = [
        ("missing header", "Google\tгуголь\n"),
        ("wrong column count", "latin\tcyrillic\nGoogle\tгуголь\textra\n"),
        ("non-latin key", "latin\tcyrillic\nГугл\tгуголь\n"),
        ("empty reading", "latin\tcyrillic\nGoogle\t\n"),
        ("padded reading", "latin\tcyrillic\nGoogle\t гуголь \n"),
        ("duplicate key", "latin\tcyrillic\nGoogle\tгуголь\ngoogle\tгугл\n"),
    ];
    for (name, source) in cases {
        let error = parse_vocabulary(source).expect_err(name);
        assert!(matches!(error, VocabularyError::Invalid { .. }), "{name}: {error}");
    }
}

#[test]
fn accepts_comments_blank_lines_and_a_byte_order_mark() {
    let words = parse_vocabulary("\u{feff}latin\tcyrillic\n\n# a comment\nGoogle\tгуголь\n")
        .expect("should parse");
    assert_eq!(words.get("google").map(String::as_str), Some("гуголь"));
}

#[test]
fn reports_a_missing_file() {
    let error = load_vocabulary_tsv("tests/data/does-not-exist.tsv").expect_err("should fail");
    assert!(matches!(error, VocabularyError::Io(_)), "{error}");
}
