//! Regex fragments and compiled patterns derived from the lexicon tables.

use fancy_regex::Regex;

use super::lexicon;
use super::re::{compile, compile_i};
use super::text::regex_alternation;
use std::sync::LazyLock;

/// All unit keys as a regex alternation, longest first.
pub(crate) static UNIT_ALT: LazyLock<String> =
    LazyLock::new(|| regex_alternation(lexicon::UNITS.iter().map(|u| u.key)));

/// All counted-noun keys as a regex alternation, longest first.
pub(crate) static COUNTED_NOUN_ALT: LazyLock<String> =
    LazyLock::new(|| regex_alternation(lexicon::COUNTED_NOUNS.iter().map(|n| n.key)));

/// Currency codes and symbols, plus `грн`, as a non-capturing alternation.
pub(crate) static CURRENCY_TOKEN_ALT: LazyLock<String> = LazyLock::new(|| {
    let mut keys: Vec<&str> = vec!["грн"];
    for entry in lexicon::CURRENCIES.iter() {
        keys.push(entry.code);
        if !entry.symbol.is_empty() {
            keys.push(entry.symbol);
        }
    }
    format!("(?:{})", regex_alternation(keys))
});

/// Currency codes alone, as an alternation.
pub(crate) static CURRENCY_CODE_ALT: LazyLock<String> =
    LazyLock::new(|| regex_alternation(lexicon::CURRENCIES.iter().map(|c| c.code)));

/// Genitive month names, in the spellings a date can use.
pub(crate) const MONTH_ALT: &str = concat!(
    "січня|січ\\.|лютого|лют\\.|березня|бер\\.|квітня|квіт\\.|травня|трав\\.|червня|черв\\.|",
    "липня|лип\\.|серпня|серп\\.|вересня|вер\\.|жовтня|жовт\\.|листопада|лист\\.|грудня|груд\\.|",
    "Січня|Лютого|Березня|Квітня|Травня|Червня|Липня|Серпня|Вересня|Жовтня|Листопада|Грудня"
);

/// A scale word such as `тис.` or `млрд`.
pub(crate) const MULTIPLIER_TOKEN: &str = r"(?:тис|млн|млрд|трлн)\.?";

/// A number that may carry a sign and a decimal part.
pub(crate) const SIGNED_NUMBER: &str = r"(?:\+|-|−|–|—)?(?:\d+(?:[.,]\d+)?|[.,]\d+)";

/// Any of the dash characters that can separate the bounds of a range.
pub(crate) const RANGE_SEPARATOR: &str = r"(?:-|−|‐|‑|‒|–|—|…)";

/// What may precede a range, so a bare hyphen inside a word is not one.
pub(crate) const RANGE_PREFIX: &str = r"(^|[\s(\[{:;,.!?=]|(?:[-–—]\s+))";

/// `15–17 травня 2024 року`
pub(crate) static DATE_DAY_RANGE_RE: LazyLock<Regex> = LazyLock::new(|| {
    compile(&format!(
        concat!(
            r"\b(\d{{1,2}})\s*(?:-|−|–|—)\s*(\d{{1,2}})\s+({})\s+(\d{{3,4}})",
            r"(?:\s+року(?![А-Яа-яЄєІіЇїҐґ])|\s*р\.(?![а-яіїєґ]))?"
        ),
        MONTH_ALT
    ))
});

/// `15 травня 2024 року`
pub(crate) static DATE_SPELLED_RE: LazyLock<Regex> = LazyLock::new(|| {
    compile_i(&format!(
        concat!(
            r"\b(\d{{1,2}})\s+({})\s+(\d{{3,4}})",
            r"(?:\s+року(?![А-Яа-яЄєІіЇїҐґ])|\s*р\.(?![а-яіїєґ]))?"
        ),
        MONTH_ALT
    ))
});

/// A preposition that governs the case of the number after it.
pub(crate) static CASE_PREP_RE: LazyLock<Regex> = LazyLock::new(|| {
    compile(&format!(
        concat!(
            r"(^|[^А-Яа-яЄєІіЇїҐґ-])(Близько|близько|Після|після|Протягом|протягом|Впродовж|впродовж|",
            r"Упродовж|упродовж|Менше|менше|Більше|більше|Серед|серед|Перед|перед|Між|між|Над|над|",
            r"Під|під|При|при|Без|без|Від|від|До|до|Із|із|З|з|Об|об|К|к|О|о)",
            r"\s+(\d+)(?:\s*({})(\.?))?(?!\s*%)(?![\d.,:%–—-])(?![A-Za-zА-Яа-яЄєІіЇїҐґ])"
        ),
        *UNIT_ALT
    ))
});

/// `понад 500 користувачів`
pub(crate) static COUNTED_PONAD_RE: LazyLock<Regex> = LazyLock::new(|| {
    compile(&format!(
        r"(^|[^А-Яа-яЄєІіЇїҐґ\d])(Понад|понад)\s+([1-9]\d{{0,5}})\s+({})(?![А-Яа-яЄєІіЇїҐґ])",
        *COUNTED_NOUN_ALT
    ))
});

/// A preposition that puts the counted noun into the genitive.
pub(crate) static COUNTED_GENITIVE_RE: LazyLock<Regex> = LazyLock::new(|| {
    compile(&format!(
        concat!(
            r"(^|[^А-Яа-яЄєІіЇїҐґ\d])(Близько|близько|Більше|більше|Менше|менше|Серед|серед|До|до|",
            r"Від|від|Без|без|Після|після|Протягом|протягом|Впродовж|впродовж|Упродовж|упродовж|Із|із)",
            r"\s+([1-9]\d{{0,5}})\s+({})(?![А-Яа-яЄєІіЇїҐґ])"
        ),
        *COUNTED_NOUN_ALT
    ))
});

/// A bare count followed by a counted noun.
pub(crate) static COUNTED_NOUNS_RE: LazyLock<Regex> = LazyLock::new(|| {
    compile_i(&format!(
        r"(^|[^А-Яа-яЄєІіЇїҐґ\d/])([1-9]\d{{0,5}})\s+({})(?![А-Яа-яЄєІіЇїҐґ])",
        *COUNTED_NOUN_ALT
    ))
});

/// A quantity followed by a unit of measure.
pub(crate) static MEASUREMENTS_RE: LazyLock<Regex> = LazyLock::new(|| {
    compile(&format!(
        r"(^|[^\d.,+\-])([+\-]?\d+(?:[.,]\d+)?)\s*({})(\.?)(?![A-Za-zА-Яа-яЄєІіЇїҐґ])",
        *UNIT_ALT
    ))
});

/// `$5 млн`
pub(crate) static SYMBOL_CURRENCY_PREFIX_RE: LazyLock<Regex> = LazyLock::new(|| {
    compile(&format!(r"({})\s*(\d+(?:[.,]\d+)?)\s*({MULTIPLIER_TOKEN})", *CURRENCY_TOKEN_ALT))
});

/// `5 млн $`
pub(crate) static SYMBOL_CURRENCY_SUFFIX_RE: LazyLock<Regex> = LazyLock::new(|| {
    compile(&format!(r"(\d+(?:[.,]\d+)?)\s*({MULTIPLIER_TOKEN})\s*({})", *CURRENCY_TOKEN_ALT))
});
