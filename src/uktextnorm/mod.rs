//! Ukrainian text normalization: numbers, dates, units, currencies,
//! abbreviations and other machine-readable spellings rewritten as words.
//!
//! ```
//! use normalize_uk::uktextnorm::{normalize, normalize_preset, NormalizePreset};
//!
//! assert_eq!(normalize("5 кг"), "п'ять кілограмів");
//! let spoken = normalize_preset("5–7 кг", NormalizePreset::TtsFriendly);
//! assert!(spoken.starts_with("від"));
//! ```

mod fuzzy_match;
mod lexicon;
mod morphology;
mod numbers;
mod options;
mod passes;
mod patterns;
mod pipeline;
mod re;
mod readers;
mod temperature;
mod text;
mod uncertainty;
mod validation;
mod vocabulary;

pub use numbers::{
    number_to_ordinal_words, number_to_words, number_to_words_case, number_to_words_digit_by_digit,
    GrammaticalCase, OrdinalForm, MAX_SPELLED_NUMBER,
};
pub use options::{
    ColonStyle, CurrencySymbolPolicy, DateStyle, InputTolerance, NormalizeOptions, NormalizePreset,
    NumericDateOrder, PhoneStyle, QuoteStyle, RangeStyle, SymbolStyle,
};
pub use passes::{expand_abbreviations, normalize_abbreviations, transliterate_to_cyrillic};
pub use pipeline::{normalize, normalize_preset, normalize_with};
pub use uncertainty::{
    flag_uncertain, flag_uncertain_with, UncertainSpan, UncertaintyCategory, UncertaintySeverity,
};
pub use vocabulary::{
    load_asr_vocabulary_tsv, load_vocabulary_tsv, parse_asr_vocabulary, parse_vocabulary,
    VocabularyError,
};
