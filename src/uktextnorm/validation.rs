//! Checksum and calendar validation for the identifiers the normalizer reads.

use once_cell::sync::Lazy;
use std::collections::HashMap;

use super::text::compact_ascii_alnum_upper;

/// Converts a Roman numeral to its value. Unknown characters count as zero.
pub(crate) fn roman_to_int(s: &str) -> u64 {
    let value = |c: char| match c {
        'I' => 1,
        'V' => 5,
        'X' => 10,
        'L' => 50,
        'C' => 100,
        'D' => 500,
        'M' => 1000,
        _ => 0,
    };
    let mut total: i64 = 0;
    let mut prev = 0;
    for c in s.chars().rev() {
        let v = value(c);
        total += if v < prev { -v } else { v };
        prev = v;
    }
    total.max(0) as u64
}

/// True when `s` is a well-formed Roman numeral in the classic subtractive style.
pub(crate) fn valid_roman(s: &str) -> bool {
    static RE: Lazy<fancy_regex::Regex> = Lazy::new(|| {
        fancy_regex::Regex::new(r"^M{0,4}(?:CM|CD|D?C{0,3})(?:XC|XL|L?X{0,3})(?:IX|IV|V?I{0,3})$")
            .expect("valid pattern")
    });
    RE.is_match(s).unwrap_or(false)
}

fn is_leap(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// True when day/month/year name a real calendar date.
pub(crate) fn is_valid_date(day: i32, month: i32, year: i32) -> bool {
    if !(1..=12).contains(&month) || day < 1 {
        return false;
    }
    #[rustfmt::skip]
    const LENGTHS: [i32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let max_day = if month == 2 && is_leap(year) { 29 } else { LENGTHS[(month - 1) as usize] };
    day <= max_day
}

/// True when `week` is a real ISO-8601 week number for `year`.
///
/// Only long years, whose 1 January is a Thursday (or a Wednesday in a leap
/// year), have a 53rd week.
pub(crate) fn is_valid_iso_week(year: i32, week: i32) -> bool {
    if !(1..=53).contains(&week) {
        return false;
    }
    if week <= 52 {
        return true;
    }
    let p = year - 1;
    let january_first_weekday = (p + p / 4 - p / 100 + p / 400 + 1) % 7;
    january_first_weekday == 4 || (january_first_weekday == 3 && is_leap(year))
}

/// Reads a digit, or 10 for a trailing `X`, used by the ISBN-10 and ISSN sums.
fn check_digit(ch: char, allow_x: bool) -> Option<u32> {
    match ch {
        '0'..='9' => Some(ch as u32 - '0' as u32),
        'X' if allow_x => Some(10),
        _ => None,
    }
}

pub(crate) fn valid_isbn(value: &str) -> bool {
    let compact = compact_ascii_alnum_upper(value);
    if compact.len() == 10 {
        let mut sum = 0;
        for (i, ch) in compact.chars().enumerate() {
            match check_digit(ch, i == 9) {
                Some(digit) => sum += (10 - i as u32) * digit,
                None => return false,
            }
        }
        return sum % 11 == 0;
    }
    if compact.len() == 13
        && compact.bytes().all(|b| b.is_ascii_digit())
        && (compact.starts_with("978") || compact.starts_with("979"))
    {
        let sum: u32 = compact
            .bytes()
            .enumerate()
            .map(|(i, b)| u32::from(b - b'0') * if i % 2 == 0 { 1 } else { 3 })
            .sum();
        return sum % 10 == 0;
    }
    false
}

pub(crate) fn valid_issn(value: &str) -> bool {
    let compact = compact_ascii_alnum_upper(value);
    if compact.len() != 8 {
        return false;
    }
    let mut sum = 0;
    for (i, ch) in compact.chars().enumerate() {
        match check_digit(ch, i == 7) {
            Some(digit) => sum += (8 - i as u32) * digit,
            None => return false,
        }
    }
    sum % 11 == 0
}

/// Expected IBAN lengths per country, used to reject truncated numbers that
/// would still pass the mod-97 check.
#[rustfmt::skip]
static IBAN_LENGTHS: Lazy<HashMap<&'static str, usize>> = Lazy::new(|| {
    [
        ("AL", 28), ("AD", 24), ("AT", 20), ("AZ", 28), ("BH", 22), ("BE", 16), ("BA", 20),
        ("BR", 29), ("BG", 22), ("CR", 22), ("HR", 21), ("CY", 28), ("CZ", 24), ("DK", 18),
        ("DO", 28), ("EE", 20), ("FO", 18), ("FI", 18), ("FR", 27), ("GE", 22), ("DE", 22),
        ("GI", 23), ("GR", 27), ("GL", 18), ("GT", 28), ("HU", 28), ("IS", 26), ("IE", 22),
        ("IL", 23), ("IT", 27), ("JO", 30), ("KZ", 20), ("XK", 20), ("KW", 30), ("LV", 21),
        ("LB", 28), ("LI", 21), ("LT", 20), ("LU", 20), ("MT", 31), ("MR", 27), ("MU", 30),
        ("MC", 27), ("MD", 24), ("ME", 22), ("NL", 18), ("MK", 19), ("NO", 15), ("PK", 24),
        ("PS", 29), ("PL", 28), ("PT", 25), ("QA", 29), ("RO", 24), ("LC", 32), ("SM", 27),
        ("ST", 25), ("SA", 24), ("RS", 22), ("SC", 31), ("SK", 24), ("SI", 19), ("ES", 24),
        ("SE", 24), ("CH", 21), ("TL", 23), ("TN", 24), ("TR", 26), ("UA", 29), ("AE", 23),
        ("GB", 22), ("VA", 22), ("VG", 24),
    ]
    .into_iter()
    .collect()
});

pub(crate) fn valid_iban(value: &str) -> bool {
    let compact = compact_ascii_alnum_upper(value);
    let bytes = compact.as_bytes();
    if !(15..=34).contains(&compact.len())
        || !bytes[0].is_ascii_uppercase()
        || !bytes[1].is_ascii_uppercase()
        || !bytes[2].is_ascii_digit()
        || !bytes[3].is_ascii_digit()
    {
        return false;
    }
    if IBAN_LENGTHS.get(&compact[..2]).is_some_and(|&len| compact.len() != len) {
        return false;
    }
    // The mod-97 check runs over the number rotated so the country code trails.
    let mut remainder: u32 = 0;
    let rotated = bytes[4..].iter().chain(&bytes[..4]);
    for &b in rotated {
        remainder = if b.is_ascii_digit() {
            (remainder * 10 + u32::from(b - b'0')) % 97
        } else if b.is_ascii_uppercase() {
            (remainder * 100 + u32::from(b - b'A') + 10) % 97
        } else {
            return false;
        };
    }
    remainder == 1
}

pub(crate) fn valid_luhn(value: &str) -> bool {
    let mut digits = Vec::new();
    for b in value.bytes() {
        if b.is_ascii_digit() {
            digits.push(b - b'0');
        } else if b != b' ' && b != b'-' {
            return false;
        }
    }
    if !(12..=19).contains(&digits.len()) {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            let d = u32::from(d);
            if i % 2 == 1 {
                if d * 2 > 9 {
                    d * 2 - 9
                } else {
                    d * 2
                }
            } else {
                d
            }
        })
        .sum();
    sum % 10 == 0
}

pub(crate) fn valid_vin_checksum(value: &str) -> bool {
    let compact = compact_ascii_alnum_upper(value);
    if compact.len() != 17 || compact.contains(['I', 'O', 'Q']) {
        return false;
    }
    // The check digit is only mandatory for North American VINs.
    if !matches!(compact.as_bytes()[0], b'1'..=b'5') {
        return true;
    }
    const WEIGHTS: [u32; 17] = [8, 7, 6, 5, 4, 3, 2, 10, 0, 9, 8, 7, 6, 5, 4, 3, 2];
    let transliterate = |ch: char| -> Option<u32> {
        match ch {
            '0'..='9' => Some(ch as u32 - '0' as u32),
            'A'..='H' => Some(ch as u32 - 'A' as u32 + 1),
            'J'..='N' => Some(ch as u32 - 'J' as u32 + 1),
            'P' => Some(7),
            'R' => Some(9),
            'S'..='Z' => Some(ch as u32 - 'S' as u32 + 2),
            _ => None,
        }
    };
    let mut sum = 0;
    for (i, ch) in compact.chars().enumerate() {
        match transliterate(ch) {
            Some(v) => sum += v * WEIGHTS[i],
            None => return false,
        }
    }
    let remainder = sum % 11;
    let expected =
        if remainder == 10 { 'X' } else { char::from_digit(remainder, 10).unwrap_or('0') };
    compact.chars().nth(8) == Some(expected)
}

pub(crate) fn valid_uuid_variant(value: &str) -> bool {
    let compact = compact_ascii_alnum_upper(value);
    if compact.len() != 32 || !compact.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    // The nil UUID has no version or variant bits.
    if compact.bytes().all(|b| b == b'0') {
        return true;
    }
    let bytes = compact.as_bytes();
    matches!(bytes[12], b'1'..=b'8') && b"89AB".contains(&bytes[16])
}

pub(crate) fn valid_hash_length(algorithm: &str, value: &str) -> bool {
    static LENGTHS: Lazy<HashMap<&'static str, usize>> = Lazy::new(|| {
        [
            ("MD5", 32),
            ("SHA1", 40),
            ("SHA224", 56),
            ("SHA256", 64),
            ("SHA384", 96),
            ("SHA512", 128),
            ("SHA3256", 64),
            ("SHA3512", 128),
            ("BLAKE2S", 64),
            ("BLAKE2B", 128),
        ]
        .into_iter()
        .collect()
    });
    let compact = compact_ascii_alnum_upper(value);
    LENGTHS
        .get(compact_ascii_alnum_upper(algorithm).as_str())
        .is_some_and(|&len| compact.len() == len && compact.bytes().all(|b| b.is_ascii_hexdigit()))
}
