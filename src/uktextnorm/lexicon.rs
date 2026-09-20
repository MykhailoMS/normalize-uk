//! The lexicon tables, parsed once from the TSV files under `data/lexicons/`.
//!
//! The TSV files are embedded at compile time and parsed on first use. Row
//! order is preserved, because several tables are matched longest-key-first and
//! a few (abbreviations) rely on the file order directly.

use once_cell::sync::Lazy;

/// Grammatical gender of a lexicon entry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Gender {
    Masculine,
    Feminine,
    Neuter,
}

impl Gender {
    fn parse(value: &str) -> Self {
        match value {
            "f" => Gender::Feminine,
            "n" => Gender::Neuter,
            _ => Gender::Masculine,
        }
    }
}

/// The one/few/many forms a Ukrainian count takes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Forms {
    pub one: &'static str,
    pub few: &'static str,
    pub many: &'static str,
}

/// A unit of measure and how it is read after a number.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Unit {
    pub key: &'static str,
    pub forms: Forms,
    /// The form used after a decimal quantity (`2,5 метра`).
    pub decimal: &'static str,
    pub gender: Gender,
}

/// A noun that can be counted, with its agreement forms.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CountedNoun {
    pub key: &'static str,
    pub forms: Forms,
    pub gender: Gender,
}

/// A cryptocurrency or finance ticker with a natural Ukrainian reading.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FinanceUnit {
    pub code: &'static str,
    pub forms: Forms,
    pub decimal: &'static str,
    pub feminine: bool,
}

/// An ISO 4217 currency and its Ukrainian main and subunit readings.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Currency {
    pub code: &'static str,
    /// The currency symbol, or empty when the currency has none.
    pub symbol: &'static str,
    /// A regex fragment matching Ukrainian word forms after an amount.
    pub word_re: &'static str,
    pub main: Forms,
    pub main_feminine: bool,
    pub sub: Forms,
    pub sub_feminine: bool,
    /// Whether a bare `<digits> <symbol>` spelling should also be matched.
    pub trailing_symbol: bool,
    /// The ISO minor-unit exponent: 0, 2, 3 or 4.
    pub minor_digits: u32,
}

/// A `key -> expansion` pair.
pub(crate) type Pair = (&'static str, &'static str);

/// Splits a TSV table into its data rows, dropping comments and the header.
fn rows(source: &'static str, expected_header: &str) -> Vec<Vec<&'static str>> {
    let mut lines = source
        .lines()
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    let header = lines.next().expect("lexicon table is empty");
    assert_eq!(header, expected_header, "unexpected lexicon header");
    let width = expected_header.split('\t').count();
    lines
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            assert_eq!(fields.len(), width, "wrong column count in lexicon row: {line:?}");
            fields
        })
        .collect()
}

fn pairs(source: &'static str, header: &str) -> Vec<Pair> {
    rows(source, header).into_iter().map(|r| (r[0], r[1])).collect()
}

macro_rules! table {
    ($name:ident, $ty:ty, $file:literal, $header:literal, $row:expr) => {
        pub(crate) static $name: Lazy<Vec<$ty>> = Lazy::new(|| {
            rows(include_str!(concat!("../../data/lexicons/", $file)), $header)
                .into_iter()
                .map($row)
                .collect()
        });
    };
}

macro_rules! pair_table {
    ($name:ident, $file:literal, $header:literal) => {
        pub(crate) static $name: Lazy<Vec<Pair>> =
            Lazy::new(|| pairs(include_str!(concat!("../../data/lexicons/", $file)), $header));
    };
}

table!(UNITS, Unit, "units.tsv", "key\tone\tfew\tmany\tdecimal\tgender", |r| Unit {
    key: r[0],
    forms: Forms { one: r[1], few: r[2], many: r[3] },
    decimal: r[4],
    gender: Gender::parse(r[5]),
});

table!(COUNTED_NOUNS, CountedNoun, "counted_nouns.tsv", "key\tone\tfew\tmany\tgender", |r| {
    CountedNoun {
        key: r[0],
        forms: Forms { one: r[1], few: r[2], many: r[3] },
        gender: Gender::parse(r[4]),
    }
});

table!(
    FINANCE_UNITS,
    FinanceUnit,
    "finance_units.tsv",
    "code\tone\tfew\tmany\tdecimal\tfeminine",
    |r| FinanceUnit {
        code: r[0],
        forms: Forms { one: r[1], few: r[2], many: r[3] },
        decimal: r[4],
        feminine: r[5] == "1",
    }
);

table!(
    CURRENCIES,
    Currency,
    "currencies.tsv",
    "code\tsymbol\tword_re\tmain_one\tmain_few\tmain_many\tmain_fem\tsub_one\tsub_few\tsub_many\tsub_fem\ttrailing_symbol\tminor_digits",
    |r| Currency {
        code: r[0],
        symbol: r[1],
        word_re: r[2],
        main: Forms { one: r[3], few: r[4], many: r[5] },
        main_feminine: r[6] == "1",
        sub: Forms { one: r[7], few: r[8], many: r[9] },
        sub_feminine: r[10] == "1",
        trailing_symbol: r[11] == "1",
        minor_digits: r[12].parse().expect("minor_digits must be a number"),
    }
);

/// Counted nouns in the instrumental or locative case, keyed by surface form.
pub(crate) static COUNTED_OBLIQUE: Lazy<Vec<Pair>> = Lazy::new(|| {
    pairs(include_str!("../../data/lexicons/counted_oblique.tsv"), "key\tgrammatical_case")
});

pair_table!(ACRONYMS, "acronyms.tsv", "acronym\texpansion");
pair_table!(ABBREVIATIONS, "abbreviations.tsv", "key\texpansion");
pair_table!(BRANDS, "brands.tsv", "latin\tcyrillic");
pair_table!(ENGLISH_WORDS, "english_words.tsv", "latin\tcyrillic");

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Every table parses, and its first column is a unique key.
    ///
    /// The reference implementation enforced this in a build-time code
    /// generator; parsing at runtime moves the check here.
    #[test]
    fn keys_are_unique() {
        fn unique<'a>(name: &str, keys: impl Iterator<Item = &'a str>) {
            let mut seen = HashSet::new();
            for key in keys {
                assert!(seen.insert(key), "{name}: duplicate key {key:?}");
            }
        }
        unique("units", UNITS.iter().map(|u| u.key));
        unique("counted_nouns", COUNTED_NOUNS.iter().map(|n| n.key));
        unique("counted_oblique", COUNTED_OBLIQUE.iter().map(|&(k, _)| k));
        unique("finance_units", FINANCE_UNITS.iter().map(|u| u.code));
        unique("currencies", CURRENCIES.iter().map(|c| c.code));
        unique("acronyms", ACRONYMS.iter().map(|&(k, _)| k));
        unique("abbreviations", ABBREVIATIONS.iter().map(|&(k, _)| k));
        unique("brands", BRANDS.iter().map(|&(k, _)| k));
        unique("english_words", ENGLISH_WORDS.iter().map(|&(k, _)| k));
    }

    #[test]
    fn column_values_are_in_range() {
        for currency in CURRENCIES.iter() {
            assert!(
                matches!(currency.minor_digits, 0 | 2 | 3 | 4),
                "{}: minor_digits must be 0, 2, 3 or 4",
                currency.code
            );
        }
        for &(key, case) in COUNTED_OBLIQUE.iter() {
            assert!(
                matches!(case, "instr" | "prep"),
                "{key}: grammatical case must be instr or prep"
            );
        }
    }

    #[test]
    fn required_columns_are_present() {
        for unit in UNITS.iter() {
            for (name, value) in [
                ("key", unit.key),
                ("one", unit.forms.one),
                ("few", unit.forms.few),
                ("many", unit.forms.many),
                ("decimal", unit.decimal),
            ] {
                assert!(!value.is_empty(), "unit {:?}: {name} is empty", unit.key);
            }
        }
        for currency in CURRENCIES.iter() {
            // `symbol` and `word_re` are optional; the readings are not.
            for (name, value) in [
                ("main_one", currency.main.one),
                ("main_many", currency.main.many),
                ("sub_one", currency.sub.one),
                ("sub_many", currency.sub.many),
            ] {
                assert!(!value.is_empty(), "currency {}: {name} is empty", currency.code);
            }
        }
    }
}
