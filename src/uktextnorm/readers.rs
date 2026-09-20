//! Readers that turn identifiers, amounts and phone numbers into words.

use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::lexicon::{self, FinanceUnit, Gender, Unit};
use super::morphology::{feminine_last, plural, plural_of, PRONUNCIATION};
use super::numbers::{
    decimal_to_words_or_digits, number_digits_or_words, number_to_words,
    number_to_words_digit_by_digit, ordinal_words,
};
use super::text::{is_uk, join, parse_u64, split_words, try_parse_u64, upper_cp};
use super::validation::{roman_to_int, valid_roman};
use super::PhoneStyle;

/// Units of measure keyed by their written abbreviation.
pub(crate) static MEASUREMENTS: Lazy<HashMap<&'static str, &'static Unit>> =
    Lazy::new(|| lexicon::UNITS.iter().map(|u| (u.key, u)).collect());

/// Finance and cryptocurrency tickers keyed by code.
pub(crate) static FINANCE_UNITS: Lazy<HashMap<&'static str, &'static FinanceUnit>> =
    Lazy::new(|| lexicon::FINANCE_UNITS.iter().map(|u| (u.code, u)).collect());

/// Counted nouns keyed by surface form.
pub(crate) static COUNTED_NOUNS: Lazy<HashMap<&'static str, &'static lexicon::CountedNoun>> =
    Lazy::new(|| lexicon::COUNTED_NOUNS.iter().map(|n| (n.key, n)).collect());

/// Oblique counted-noun forms mapped to their case (`instr` or `prep`).
pub(crate) static COUNTED_OBLIQUE: Lazy<HashMap<&'static str, &'static str>> =
    Lazy::new(|| lexicon::COUNTED_OBLIQUE.iter().copied().collect());

/// Latin words with a preferred Ukrainian reading: brands first, then the
/// general English word list, which never overrides a brand.
pub(crate) static ENGLISH_WORDS: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    let mut out: HashMap<&str, &str> = HashMap::new();
    for &(latin, cyrillic) in lexicon::BRANDS.iter().chain(lexicon::ENGLISH_WORDS.iter()) {
        out.entry(latin).or_insert(cyrillic);
    }
    out
});

/// Abbreviations keyed by their whitespace-free lowercase form.
pub(crate) static ABBREVIATIONS: Lazy<HashMap<String, &'static str>> = Lazy::new(|| {
    lexicon::ABBREVIATIONS
        .iter()
        .map(|&(key, expansion)| (super::text::compact_spaces_lower(key), expansion))
        .collect()
});

/// Reads a quantity followed by a unit, agreeing the number with the unit.
pub(crate) fn read_measurement_quantity(num: &str, unit: &Unit) -> String {
    let mut num = num;
    let mut sign = "";
    if let Some(rest) = num.strip_prefix('-') {
        sign = "мінус ";
        num = rest;
    } else if let Some(rest) = num.strip_prefix('+') {
        sign = "плюс ";
        num = rest;
    }
    if let Some(pos) = num.find(['.', ',']) {
        let integer = if num[..pos].is_empty() { "0" } else { &num[..pos] };
        return format!(
            "{sign}{} {}",
            decimal_to_words_or_digits(integer, &num[pos + 1..]),
            unit.decimal
        );
    }
    let Some(n) = try_parse_u64(num) else {
        return format!("{sign}{} {}", number_to_words_digit_by_digit(num), unit.forms.many);
    };
    let mut words = split_words(&number_to_words(n));
    if unit.gender == Gender::Feminine {
        feminine_last(&mut words);
    }
    format!("{sign}{} {}", join(&words), plural(n, &unit.forms))
}

/// Reads an amount followed by a finance ticker.
pub(crate) fn finance_amount_words(amount: &str, unit: &FinanceUnit) -> String {
    finance_amount_words_parts(
        amount,
        unit.forms.one,
        unit.forms.few,
        unit.forms.many,
        unit.decimal,
        unit.feminine,
    )
}

/// [`finance_amount_words`] over forms that are not `'static`, used when an
/// unknown ticker is spelled out and reused as its own unit name.
pub(crate) fn finance_amount_words_parts(
    amount: &str,
    one: &str,
    few: &str,
    many: &str,
    decimal: &str,
    feminine: bool,
) -> String {
    let amount = amount.replace(' ', "");
    if let Some(pos) = amount.find(['.', ',']) {
        return format!(
            "{} {}",
            decimal_to_words_or_digits(&amount[..pos], &amount[pos + 1..]),
            decimal
        );
    }
    let n = parse_u64(&amount);
    let mut words = split_words(&number_to_words(n));
    if feminine {
        feminine_last(&mut words);
    }
    format!("{} {}", join(&words), plural_of(n, one, few, many))
}

/// How each Latin letter is named when an identifier is spelled out.
#[rustfmt::skip]
static LATIN_LETTERS: Lazy<HashMap<char, &'static str>> = Lazy::new(|| {
    [
        ('A', "ей"), ('B', "бі"), ('C', "сі"), ('D', "ді"), ('E', "і"), ('F', "еф"),
        ('G', "джі"), ('H', "ейч"), ('I', "ай"), ('J', "джей"), ('K', "кей"), ('L', "ел"),
        ('M', "ем"), ('N', "ен"), ('O', "оу"), ('P', "пі"), ('Q', "к'ю"), ('R', "ар"),
        ('S', "ес"), ('T', "ті"), ('U', "ю"), ('V', "ві"), ('W', "дабл ю"), ('X', "екс"),
        ('Y', "вай"), ('Z', "зед"),
    ]
    .into_iter()
    .collect()
});

/// Spells out a run of letters one at a time.
pub(crate) fn spell_identifier_letters(letters: &str) -> String {
    let mut parts = Vec::new();
    for cp in letters.chars() {
        if cp.is_ascii() {
            if let Some(name) = LATIN_LETTERS.get(&cp.to_ascii_uppercase()) {
                parts.push((*name).to_owned());
            }
        } else {
            let upper = upper_cp(cp).to_string();
            if let Some(name) = PRONUNCIATION.get(upper.as_str()) {
                parts.push((*name).to_owned());
            }
        }
    }
    join(&parts)
}

/// Reads a run of digits inside an identifier: short groups as a number, long
/// or zero-padded ones digit by digit.
pub(crate) fn read_identifier_number(digits: &str) -> String {
    if digits.len() > 4 || (digits.len() > 1 && digits.starts_with('0')) {
        return number_to_words_digit_by_digit(digits);
    }
    number_to_words(parse_u64(digits))
}

/// Reads a run of letters as a Roman numeral, when it is one.
pub(crate) fn read_roman_identifier_segment(letters: &str) -> Option<String> {
    let mut roman = String::with_capacity(letters.len());
    for ch in letters.chars() {
        let ch = ch.to_ascii_uppercase();
        if !"IVXLCDM".contains(ch) {
            return None;
        }
        roman.push(ch);
    }
    if roman.is_empty() || !valid_roman(&roman) {
        return None;
    }
    Some(ordinal_words(roman_to_int(&roman), "nom_m"))
}

/// Reads one alphanumeric run, alternating between digit groups and letters.
pub(crate) fn read_identifier_segment(segment: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut digits = String::new();
    let mut letters = String::new();
    macro_rules! flush_digits {
        () => {
            if !digits.is_empty() {
                parts.push(read_identifier_number(&digits));
                digits.clear();
            }
        };
    }
    macro_rules! flush_letters {
        () => {
            if !letters.is_empty() {
                parts.push(
                    read_roman_identifier_segment(&letters)
                        .unwrap_or_else(|| spell_identifier_letters(&letters)),
                );
                letters.clear();
            }
        };
    }
    for cp in segment.chars() {
        if cp.is_ascii_digit() {
            flush_letters!();
            digits.push(cp);
        } else if cp.is_ascii_alphabetic() || is_uk(cp) {
            flush_digits!();
            letters.push(cp);
        } else {
            flush_digits!();
            flush_letters!();
        }
    }
    flush_digits!();
    flush_letters!();
    join(&parts)
}

/// Reads an identifier, naming the separators between its segments.
pub(crate) fn read_structured_identifier(value: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    for cp in value.chars() {
        if cp.is_ascii_alphanumeric() || is_uk(cp) {
            current.push(cp);
            continue;
        }
        if !current.is_empty() {
            parts.push(read_identifier_segment(&current));
            current.clear();
        }
        match cp {
            '/' => parts.push("слеш".to_owned()),
            '-' | '‑' | '–' | '—' => parts.push("дефіс".to_owned()),
            _ => {}
        }
    }
    if !current.is_empty() {
        parts.push(read_identifier_segment(&current));
    }
    join(&parts)
}

/// Reads a dotted number such as a version or a clause reference.
pub(crate) fn read_dotted(num: &str) -> String {
    let parts: Vec<String> = num
        .split('.')
        .map(|p| {
            if p.len() > 1 && p.starts_with('0') {
                number_to_words_digit_by_digit(p)
            } else {
                number_digits_or_words(p)
            }
        })
        .collect();
    parts.join(" крапка ")
}

/// Reads a phone number, grouping digits the way the style requests.
pub(crate) fn normalize_phone_number(phone: &str, style: PhoneStyle) -> String {
    let international_access = phone.starts_with("00");
    let mut digits = String::new();
    let mut groups: Vec<String> = Vec::new();
    let mut current = String::new();
    for ch in phone.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            current.push(ch);
        } else if !current.is_empty() {
            groups.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }
    if international_access {
        digits.drain(..2.min(digits.len()));
        if let Some(first) = groups.first_mut() {
            first.drain(..2.min(first.len()));
            if first.is_empty() {
                groups.remove(0);
            }
        }
    }
    // A bare Ukrainian mobile number gains its country code.
    if digits.len() == 10 && digits.starts_with('0') {
        digits.insert_str(0, "38");
    }

    // Reads a group as a number, unless it is long or zero-padded.
    let read_group = |group: &str| {
        if group.len() > 3 || (group.len() > 1 && group.starts_with('0')) {
            number_to_words_digit_by_digit(group)
        } else {
            number_to_words(parse_u64(group))
        }
    };

    if digits.len() != 12 || !digits.starts_with("380") {
        if (!phone.starts_with('+') && !international_access) || !(7..=15).contains(&digits.len()) {
            return phone.to_owned();
        }
        let mut parts = vec!["плюс".to_owned()];
        if style == PhoneStyle::DigitByDigit || groups.len() < 2 {
            parts.push(number_to_words_digit_by_digit(&digits));
        } else {
            parts.extend(groups.iter().map(|g| read_group(g)));
        }
        return join(&parts);
    }
    if style == PhoneStyle::DigitByDigit {
        return format!("плюс {}", number_to_words_digit_by_digit(&digits));
    }
    // +380 AA BBB CC DD
    let mut parts = vec!["плюс".to_owned(), "триста вісімдесят".to_owned()];
    for (start, len) in [(3, 2), (5, 3), (8, 2), (10, 2)] {
        let seg = &digits[start..start + len];
        parts.push(if seg.len() > 1 && seg.starts_with('0') {
            number_to_words_digit_by_digit(seg)
        } else {
            number_to_words(parse_u64(seg))
        });
    }
    join(&parts)
}
