//! Reporting the places where normalization had to guess.

use fancy_regex::{Captures, Regex};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use super::fuzzy_match;
use super::lexicon;
use super::patterns::UNIT_ALT;
use super::re::{cap, cap_start, compile, compile_i, each, matched, whole};
use super::readers::{COUNTED_NOUNS, COUNTED_OBLIQUE, ENGLISH_WORDS, MEASUREMENTS};
use super::text::{
    is_ascii_acronym, is_latin, is_uk, is_upper_uk, is_word_joiner, lower_text, parse_i32,
    uncertain_word_spans,
};
use super::validation::{
    is_valid_date, is_valid_iso_week, valid_hash_length, valid_iban, valid_isbn, valid_issn,
    valid_luhn, valid_roman, valid_uuid_variant, valid_vin_checksum,
};
use super::{ColonStyle, CurrencySymbolPolicy, InputTolerance, NormalizeOptions, NumericDateOrder};

/// What kind of ambiguity a span reports.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[allow(missing_docs)]
pub enum UncertaintyCategory {
    AmbiguousAbbreviation,
    BareNumber,
    Currency,
    Date,
    Identifier,
    ForeignWord,
    MixedScript,
    RomanNumeral,
    Unit,
    Web,
    InvalidDate,
    AmbiguousNumberGrouping,
    Agreement,
    Time,
    Fraction,
    Network,
    Scientific,
    Coordinate,
    /// The token did not match a lexicon key exactly and was resolved to the
    /// closest known reading under [`InputTolerance::Asr`](super::InputTolerance).
    ApproximateMatch,
}

/// How much the ambiguity matters.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum UncertaintySeverity {
    /// Worth knowing, but the reading is probably right.
    Info,
    /// The reading depends on a judgement call.
    Warning,
    /// The value is invalid or cannot be read.
    Error,
}

/// One place in the input where the reading is not certain.
///
/// `start` and `stop` are byte offsets, so `&source[start..stop] == text`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UncertainSpan {
    /// Byte offset of the first character.
    pub start: usize,
    /// Byte offset one past the last character.
    pub stop: usize,
    /// The text of the span.
    pub text: String,
    /// A human-readable explanation.
    pub reason: String,
    /// What kind of ambiguity this is.
    pub category: UncertaintyCategory,
    /// How much it matters.
    pub severity: UncertaintySeverity,
}

use UncertaintyCategory as Cat;
use UncertaintySeverity as Sev;

/// Collects spans, ignoring duplicates and snapping to character boundaries.
struct Collector<'t> {
    text: &'t str,
    spans: Vec<UncertainSpan>,
    seen: HashSet<(usize, usize)>,
}

impl<'t> Collector<'t> {
    fn new(text: &'t str) -> Self {
        Self { text, spans: Vec::new(), seen: HashSet::new() }
    }

    fn add(
        &mut self,
        start: usize,
        stop: usize,
        reason: impl Into<String>,
        category: Cat,
        severity: Sev,
    ) {
        let mut start = start.min(self.text.len());
        let mut stop = stop.clamp(start, self.text.len());
        while start > 0 && !self.text.is_char_boundary(start) {
            start -= 1;
        }
        while stop < self.text.len() && !self.text.is_char_boundary(stop) {
            stop += 1;
        }
        if self.seen.insert((start, stop)) {
            self.spans.push(UncertainSpan {
                start,
                stop,
                text: self.text[start..stop].to_owned(),
                reason: reason.into(),
                category,
                severity,
            });
        }
    }
}

/// The end of the whole match.
fn end_of(m: &Captures<'_, str>) -> usize {
    m.get(0).map_or(0, |g| g.end())
}

fn group_end(m: &Captures<'_, str>, index: usize) -> usize {
    m.get(index).map_or(0, |g| g.end())
}

fn decimal_value(token: &str) -> Option<f64> {
    token.replace(',', ".").parse().ok()
}

/// Words that are legitimate after a number and so are not unknown units.
#[rustfmt::skip]
static KNOWN_UNIT_WORDS: LazyLock<HashSet<String>> = LazyLock::new(|| {
    let mut out: HashSet<String> = [
        "грн", "коп", "btc", "eth", "usdt", "bnb", "у", "в", "і", "й", "та", "до", "від", "на",
        "за", "з", "із", "зі", "по", "для", "р", "рр", "тис", "млн", "млрд", "трлн", "рік",
        "року", "році", "раз", "рази", "разів",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    for entry in lexicon::CURRENCIES.iter() {
        for word in [
            entry.code,
            entry.main.one,
            entry.main.few,
            entry.main.many,
            entry.sub.one,
            entry.sub.few,
            entry.sub.many,
        ] {
            out.insert(lower_text(word));
        }
    }
    out
});

/// Roman-looking acronyms that are never numerals.
#[rustfmt::skip]
static ROMAN_STOP: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    ["CD", "DVD", "MD", "DC", "MC", "MI", "MM", "DI", "DIV", "MIX", "CIV", "LCD"]
        .into_iter()
        .collect()
});

/// Timezone names the normalizer knows how to read.
#[rustfmt::skip]
static SUPPORTED_IANA_ZONES: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "europe/kyiv", "europe/london", "europe/warsaw", "america/new_york",
        "america/los_angeles", "asia/tokyo",
    ]
    .into_iter()
    .collect()
});

/// Abbreviations with several common expansions.
#[rustfmt::skip]
static MULTISENSE: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    [
        ("р", "рік / рядок / річка"),
        ("м", "метр / місто"),
        ("с", "секунда / село / сторінка"),
        ("в", "вік / вулиця / прийменник"),
        ("кв", "квартира / квартал / квадратний"),
        ("ст", "століття / стаття / станція / сторінка"),
        ("п", "пункт / пан / поверх"),
        ("обл", "область / обліковий"),
    ]
    .into_iter()
    .collect()
});

/// Prepositions that already fix the case of the number after them.
#[rustfmt::skip]
static GOVERNORS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "близько", "понад", "менше", "більше", "від", "до", "із", "з", "без", "після", "к", "у",
        "в", "о", "об", "при", "над", "під", "перед", "між",
    ]
    .into_iter()
    .collect()
});

/// Any Cyrillic code point, as the reference implementation's byte classes mean.
const CYRILLIC: &str = r"\u{0400}-\u{04FF}";

/// Reports every place in `text` where the reading involves a judgement call.
///
/// ```
/// # use normalize_uk::uktextnorm::flag_uncertain;
/// let spans = flag_uncertain("10:30, $12");
/// assert!(!spans.is_empty());
/// ```
#[must_use]
pub fn flag_uncertain(text: &str) -> Vec<UncertainSpan> {
    flag_uncertain_impl(text, None)
}

/// Like [`flag_uncertain`], but omits the warnings the options already resolve.
#[must_use]
pub fn flag_uncertain_with(text: &str, options: &NormalizeOptions) -> Vec<UncertainSpan> {
    flag_uncertain_impl(text, Some(options))
}

fn flag_uncertain_impl(text: &str, options: Option<&NormalizeOptions>) -> Vec<UncertainSpan> {
    let mut c = Collector::new(text);

    static AMBIGUOUS_NUMERIC_DATE: LazyLock<Regex> = LazyLock::new(|| {
        compile(r"\b(?:0?[1-9]|1[0-2])[./-](?:0?[1-9]|1[0-2])[./-](?:\d{2}|\d{4})\b")
    });
    let date_order_unresolved =
        options.is_none_or(|o| o.numeric_date_order == NumericDateOrder::PreserveAmbiguous);
    if date_order_unresolved {
        each(text, &AMBIGUOUS_NUMERIC_DATE, |m| {
            c.add(
                cap_start(m, 0),
                end_of(m),
                "ambiguous numeric date order (day/month or month/day)",
                Cat::Date,
                Sev::Warning,
            );
        });
    }

    static AMBIGUOUS_COLON: LazyLock<Regex> =
        LazyLock::new(|| compile(r"(^|[^\d:])(\d{1,2}):([0-5]\d)(?![\d:])"));
    if options.is_none_or(|o| o.colon_style == ColonStyle::Contextual) {
        each(text, &AMBIGUOUS_COLON, |m| {
            let hour = parse_i32(cap(m, 2));
            let minute = parse_i32(cap(m, 3));
            if hour > 23 && !(hour == 24 && minute == 0) {
                return;
            }
            let start = cap_start(m, 2);
            c.add(
                start,
                start + cap(m, 2).len() + 1 + cap(m, 3).len(),
                "ambiguous colon pair (clock time or ratio)",
                Cat::Time,
                Sev::Warning,
            );
        });
    }

    static AMBIGUOUS_CURRENCY_SYMBOL: LazyLock<Regex> =
        LazyLock::new(|| compile(r"(?:\$|¥)\s*\d+(?:[.,]\d+)?"));
    if options.is_none_or(|o| o.currency_symbol_policy == CurrencySymbolPolicy::PreserveAmbiguous) {
        each(text, &AMBIGUOUS_CURRENCY_SYMBOL, |m| {
            c.add(
                cap_start(m, 0),
                end_of(m),
                "ambiguous currency symbol (currency depends on locale)",
                Cat::Currency,
                Sev::Warning,
            );
        });
    }

    static NUMERIC_DATE: LazyLock<Regex> =
        LazyLock::new(|| compile(r"(^|[^\d])(\d{1,2})\.(\d{1,2})\.(\d{3,4})(?![\d])"));
    each(text, &NUMERIC_DATE, |m| {
        let day = parse_i32(cap(m, 2));
        let month = parse_i32(cap(m, 3));
        let start = cap_start(m, 2);
        let stop = start + cap(m, 2).len() + 1 + cap(m, 3).len() + 1 + cap(m, 4).len();
        if !(1..=31).contains(&day) || !(1..=12).contains(&month) {
            c.add(start, stop, "invalid or ambiguous numeric date", Cat::Date, Sev::Error);
        } else if !is_valid_date(day, month, parse_i32(cap(m, 4))) {
            c.add(
                start,
                stop,
                "calendar-invalid date (day does not exist in that month)",
                Cat::InvalidDate,
                Sev::Error,
            );
        }
    });

    static INVALID_ISO_DATE: LazyLock<Regex> =
        LazyLock::new(|| compile(r"(^|[^\d])(\d{4})-(\d{1,2})(?:-(\d{1,2}))?(?!\d)"));
    each(text, &INVALID_ISO_DATE, |m| {
        let year = parse_i32(cap(m, 2));
        let month = parse_i32(cap(m, 3));
        let day = if matched(m, 4) { parse_i32(cap(m, 4)) } else { 1 };
        if (1..=12).contains(&month) && (!matched(m, 4) || is_valid_date(day, month, year)) {
            return;
        }
        c.add(cap_start(m, 2), end_of(m), "invalid ISO date", Cat::InvalidDate, Sev::Error);
    });

    static ISO_WEEK_CANDIDATE: LazyLock<Regex> =
        LazyLock::new(|| compile_i(r"\b(\d{4})-W(\d{2})(?:-(\d))?\b"));
    each(text, &ISO_WEEK_CANDIDATE, |m| {
        let week = parse_i32(cap(m, 2));
        let day = if matched(m, 3) { parse_i32(cap(m, 3)) } else { 1 };
        if is_valid_iso_week(parse_i32(cap(m, 1)), week) && (1..=7).contains(&day) {
            return;
        }
        c.add(cap_start(m, 0), end_of(m), "invalid ISO week date", Cat::InvalidDate, Sev::Error);
    });

    static ISO_ORDINAL_CANDIDATE: LazyLock<Regex> =
        LazyLock::new(|| compile(r"\b(\d{4})-(\d{3})\b"));
    each(text, &ISO_ORDINAL_CANDIDATE, |m| {
        let year = parse_i32(cap(m, 1));
        let day = parse_i32(cap(m, 2));
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        if (1..=if leap { 366 } else { 365 }).contains(&day) {
            return;
        }
        c.add(cap_start(m, 0), end_of(m), "invalid ISO ordinal date", Cat::InvalidDate, Sev::Error);
    });

    static TIMEZONE_OFFSET: LazyLock<Regex> =
        LazyLock::new(|| compile_i(r"(?:UTC|GMT)\s*([+-])(\d{2}):?(\d{2})(?!\d)"));
    each(text, &TIMEZONE_OFFSET, |m| {
        let hour = parse_i32(cap(m, 2));
        let minute = parse_i32(cap(m, 3));
        if minute <= 59 && (hour < 14 || (hour == 14 && minute == 0)) {
            return;
        }
        c.add(cap_start(m, 0), end_of(m), "invalid timezone offset", Cat::Time, Sev::Error);
    });

    static BARE_TIMEZONE_OFFSET: LazyLock<Regex> =
        LazyLock::new(|| compile(r"(^|[\s(])([+-])(\d{2}):(\d{2})(?!\d)"));
    each(text, &BARE_TIMEZONE_OFFSET, |m| {
        let hour = parse_i32(cap(m, 3));
        let minute = parse_i32(cap(m, 4));
        if minute <= 59 && (hour < 14 || (hour == 14 && minute == 0)) {
            return;
        }
        let start = cap_start(m, 2);
        c.add(
            start,
            start + cap(m, 2).len() + cap(m, 3).len() + cap(m, 4).len() + 1,
            "invalid timezone offset",
            Cat::Time,
            Sev::Error,
        );
    });

    static IANA_ZONE: LazyLock<Regex> = LazyLock::new(|| {
        compile(r"(\b\d{1,2}:[0-5]\d(?::[0-5]\d)?\s+)([A-Za-z_+-]+/[A-Za-z0-9_+/-]+)\b")
    });
    each(text, &IANA_ZONE, |m| {
        if SUPPORTED_IANA_ZONES.contains(lower_text(cap(m, 2)).as_str()) {
            return;
        }
        let start = cap_start(m, 2);
        c.add(
            start,
            start + cap(m, 2).len(),
            "unrecognized IANA timezone name",
            Cat::Time,
            Sev::Warning,
        );
    });

    static SINGLE_COMMA_GROUP: LazyLock<Regex> =
        LazyLock::new(|| compile(r"(^|[^\d.,])(\d{1,3},\d{3})(?![\d])"));
    each(text, &SINGLE_COMMA_GROUP, |m| {
        let start = cap_start(m, 2);
        c.add(
            start,
            start + cap(m, 2).len(),
            "single comma group (decimal or thousands separator?)",
            Cat::AmbiguousNumberGrouping,
            Sev::Warning,
        );
    });

    static INVALID_TIME: LazyLock<Regex> =
        LazyLock::new(|| compile(r"(^|[^\d:])(\d{1,3}):(\d{2})(?::(\d{2}))?(?![\d:])"));
    each(text, &INVALID_TIME, |m| {
        let hour = parse_i32(cap(m, 2));
        let minute = parse_i32(cap(m, 3));
        let second = if matched(m, 4) { parse_i32(cap(m, 4)) } else { 0 };
        let valid = (hour <= 23 || (hour == 24 && minute == 0 && second == 0))
            && minute <= 59
            && second <= 59;
        if valid {
            return;
        }
        c.add(cap_start(m, 2), end_of(m), "invalid clock time", Cat::Time, Sev::Error);
    });

    static INVALID_AMPM: LazyLock<Regex> = LazyLock::new(|| {
        compile_i(r"(^|[^\d:])(\d{1,2}):([0-5]\d)(?::([0-5]\d))?\s*(AM|PM)(?![A-Za-z])")
    });
    each(text, &INVALID_AMPM, |m| {
        if (1..=12).contains(&parse_i32(cap(m, 2))) {
            return;
        }
        c.add(cap_start(m, 2), end_of(m), "invalid 12-hour clock time", Cat::Time, Sev::Error);
    });

    static ZERO_FRACTION: LazyLock<Regex> =
        LazyLock::new(|| compile(r"(^|[^\d/])([+\-−]?\d+/0+)(?!\d)"));
    each(text, &ZERO_FRACTION, |m| {
        let start = cap_start(m, 2);
        c.add(
            start,
            start + cap(m, 2).len(),
            "fraction has a zero denominator",
            Cat::Fraction,
            Sev::Error,
        );
    });

    static MALFORMED_SCIENTIFIC: LazyLock<Regex> =
        LazyLock::new(|| compile(r"(^|[^A-Za-z\d])([+\-]?\d+(?:[.,]\d+)?[eE][+\-]?)(?!\d)"));
    static IEEE_REVISION: LazyLock<Regex> = LazyLock::new(|| compile(r"^802\.\d{1,2}[eE]$"));
    each(text, &MALFORMED_SCIENTIFIC, |m| {
        if IEEE_REVISION.is_match(cap(m, 2)).unwrap_or(false) {
            return;
        }
        let start = cap_start(m, 2);
        c.add(
            start,
            start + cap(m, 2).len(),
            "malformed scientific notation",
            Cat::Scientific,
            Sev::Warning,
        );
    });

    static IPV4_LIKE: LazyLock<Regex> = LazyLock::new(|| {
        compile(
            r"(^|[^\d.])(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})(?:/(\d{1,3}))?(?::(\d{1,6}))?(?![\d.])",
        )
    });
    each(text, &IPV4_LIKE, |m| {
        let invalid = (2..=5).any(|i| parse_i32(cap(m, i)) > 255)
            || (matched(m, 6) && parse_i32(cap(m, 6)) > 32)
            || (matched(m, 7) && parse_i32(cap(m, 7)) > 65535);
        if !invalid {
            return;
        }
        c.add(
            cap_start(m, 2),
            end_of(m),
            "invalid IP address, CIDR prefix, or port",
            Cat::Network,
            Sev::Error,
        );
    });

    static GEO_CANDIDATE: LazyLock<Regex> = LazyLock::new(|| {
        compile_i(concat!(
            r"\bgeo\s*:\s*([+\-]?\d{1,3}(?:\.\d+)?)\s*[,;]\s*([+\-]?\d{1,3}(?:\.\d+)?)",
            r"(?:\s*[,;]\s*[+\-]?\d+(?:\.\d+)?)?"
        ))
    });
    each(text, &GEO_CANDIDATE, |m| {
        let ok = matches!(
            (decimal_value(cap(m, 1)), decimal_value(cap(m, 2))),
            (Some(lat), Some(lon)) if lat.abs() <= 90.0 && lon.abs() <= 180.0
        );
        if ok {
            return;
        }
        c.add(
            cap_start(m, 0),
            end_of(m),
            "coordinate outside latitude or longitude bounds",
            Cat::Coordinate,
            Sev::Error,
        );
    });

    static DMS_CANDIDATE: LazyLock<Regex> = LazyLock::new(|| {
        compile_i(r#"(\d{1,3})\s*°\s*(\d{1,2})\s*(?:′|')\s*(\d{1,2})\s*(?:″|")\s*([NSEW])"#)
    });
    each(text, &DMS_CANDIDATE, |m| {
        let marker = lower_text(cap(m, 4));
        let limit = if marker == "n" || marker == "s" { 90 } else { 180 };
        let degrees = parse_i32(cap(m, 1));
        let minutes = parse_i32(cap(m, 2));
        let seconds = parse_i32(cap(m, 3));
        let valid = degrees <= limit
            && minutes <= 59
            && seconds <= 59
            && (degrees < limit || (minutes == 0 && seconds == 0));
        if valid {
            return;
        }
        c.add(
            cap_start(m, 0),
            end_of(m),
            "invalid degrees, minutes, or seconds coordinate",
            Cat::Coordinate,
            Sev::Error,
        );
    });

    static ABBR: LazyLock<Regex> =
        LazyLock::new(|| compile_i(r"(^|[^А-Яа-яЄєІіЇїҐґ])(кв|обл|ст|р|м|с|в|п)\.(?![а-яіїєґ])"));
    each(text, &ABBR, |m| {
        let start = cap_start(m, 2);
        let left = text[..start].trim_end_matches(' ');
        // After a number these are units, which are read unambiguously.
        if left.ends_with(|ch: char| ch.is_ascii_digit()) {
            return;
        }
        let key = lower_text(cap(m, 2));
        let Some(senses) = MULTISENSE.get(key.as_str()) else { return };
        c.add(
            start,
            start + cap(m, 2).len() + 1,
            format!("ambiguous abbreviation ({senses})"),
            Cat::AmbiguousAbbreviation,
            Sev::Warning,
        );
    });

    // Under ASR tolerance, flag every Latin word that will be resolved
    // approximately: it misses the lexicon exactly but reaches a reading through
    // the canonical-key or fuzzy fallback. This is what keeps an approximate
    // reading visible instead of silently guessed.
    if options.is_some_and(|o| o.input_tolerance == InputTolerance::Asr) {
        static LATIN_WORD: LazyLock<Regex> = LazyLock::new(|| compile(r"\b[A-Za-z][A-Za-z'’-]*\b"));
        let vocabulary = options.map(|o| &o.vocabulary);
        each(text, &LATIN_WORD, |m| {
            let token = whole(m);
            let low = lower_text(token);
            let known_exact = ENGLISH_WORDS.contains_key(low.as_str())
                || vocabulary.is_some_and(|v| v.contains_key(&low));
            if known_exact {
                return;
            }
            let entries = ENGLISH_WORDS.iter().map(|(&k, &v)| (k, v));
            if fuzzy_match::resolve(&low, entries).is_some() {
                c.add(
                    cap_start(m, 0),
                    end_of(m),
                    "approximate match: token resolved to the closest known reading",
                    Cat::ApproximateMatch,
                    Sev::Info,
                );
            }
        });
    }

    for word in uncertain_word_spans(text) {
        let token = &text[word.start..word.stop];
        let has_latin = token.chars().any(is_latin);
        let has_non_joiner_uk = token.chars().any(|cp| is_uk(cp) && !is_word_joiner(cp));
        if has_latin && has_non_joiner_uk {
            c.add(
                word.start,
                word.stop,
                "mixed-script word (possible typo or spoofing)",
                Cat::MixedScript,
                Sev::Error,
            );
        }
        if !has_latin || has_non_joiner_uk || !token.chars().next().is_some_and(is_latin) {
            continue;
        }
        let lowered = lower_text(token);
        let known = is_ascii_acronym(token)
            || ENGLISH_WORDS.contains_key(lowered.as_str())
            || options.is_some_and(|o| o.vocabulary.contains_key(&lowered));
        if known {
            continue;
        }
        // A Latin run glued to Cyrillic text is part of a larger token.
        if text[..word.start].chars().next_back().is_some_and(is_uk) {
            continue;
        }
        if text[word.stop..].chars().next().is_some_and(is_uk) {
            continue;
        }
        c.add(
            word.start,
            word.stop,
            "foreign word (transliteration is approximate)",
            Cat::ForeignWord,
            Sev::Info,
        );
    }

    static BARE_ROMAN: LazyLock<Regex> = LazyLock::new(|| compile(r"\b[MDCLXVI]{2,}\b"));
    each(text, &BARE_ROMAN, |m| {
        let token = whole(m);
        if ROMAN_STOP.contains(token) || !valid_roman(token) {
            return;
        }
        c.add(
            cap_start(m, 0),
            end_of(m),
            "Roman numeral (case defaults to nominative)",
            Cat::RomanNumeral,
            Sev::Info,
        );
    });

    /// Reports a whole match when a checksum does not hold.
    macro_rules! checked {
        ($re:expr, $group:expr, $check:expr, $reason:expr) => {
            each(text, $re, |m| {
                if !$check(cap(m, $group)) {
                    c.add(cap_start(m, 0), end_of(m), $reason, Cat::Identifier, Sev::Error);
                }
            });
        };
    }

    static ISBN_CANDIDATE: LazyLock<Regex> = LazyLock::new(|| {
        compile_i(r"\bISBN(?:-1[03])?\s*[:№#]?\s*((?:97[89][ -]?)?[0-9Xx](?:[ -]?[0-9Xx]){8,12})\b")
    });
    checked!(&ISBN_CANDIDATE, 1, valid_isbn, "invalid ISBN checksum or length");

    static ISSN_CANDIDATE: LazyLock<Regex> =
        LazyLock::new(|| compile_i(r"\bISSN(?:-L)?\s*[:№#]?\s*(\d{4}[ -]?\d{3}[\dXx])\b"));
    checked!(&ISSN_CANDIDATE, 1, valid_issn, "invalid ISSN checksum");

    static IBAN_CANDIDATE: LazyLock<Regex> =
        LazyLock::new(|| compile_i(r"\b[A-Z]{2}[ -]?\d{2}(?:[ -]?[A-Z0-9]){11,30}\b"));
    checked!(&IBAN_CANDIDATE, 0, valid_iban, "invalid IBAN checksum or length");

    static VIN_CANDIDATE: LazyLock<Regex> =
        LazyLock::new(|| compile_i(r"\bVIN\s*[:№#]?\s*([A-HJ-NPR-Z0-9]{17})\b"));
    checked!(&VIN_CANDIDATE, 1, valid_vin_checksum, "invalid VIN checksum");

    static FULL_CARD_CANDIDATE: LazyLock<Regex> = LazyLock::new(|| {
        compile_i(concat!(
            r"(^|[^A-Za-zА-Яа-яЄєІіЇїҐґ])((?:номер\s+картки|картка|картку|картки|карта|карту)",
            r"\s+(\d(?:[ -]?\d){11,18}))(?!\d)"
        ))
    });
    each(text, &FULL_CARD_CANDIDATE, |m| {
        if valid_luhn(cap(m, 3)) {
            return;
        }
        let start = cap_start(m, 2);
        c.add(
            start,
            start + cap(m, 2).len(),
            "invalid payment-card checksum",
            Cat::Identifier,
            Sev::Error,
        );
    });

    static UUID_CANDIDATE: LazyLock<Regex> = LazyLock::new(|| {
        compile_i(
            r"\b(?:([0-9A-Fa-f]{8}(?:-[0-9A-Fa-f]{4}){3}-[0-9A-Fa-f]{12})|UUID\s*[:=]?\s*([0-9A-Fa-f]{32}))\b",
        )
    });
    each(text, &UUID_CANDIDATE, |m| {
        let value = if matched(m, 1) { cap(m, 1) } else { cap(m, 2) };
        if valid_uuid_variant(value) {
            return;
        }
        c.add(
            cap_start(m, 0),
            end_of(m),
            "invalid UUID version or variant",
            Cat::Identifier,
            Sev::Error,
        );
    });

    static HASH_CANDIDATE: LazyLock<Regex> = LazyLock::new(|| {
        compile_i(
            r"\b((?:SHA-?(?:1|224|256|384|512)|SHA3-?(?:256|512)|BLAKE2[bs]|MD5))\s*[:=]?\s*([0-9A-Fa-f]{1,128})\b",
        )
    });
    each(text, &HASH_CANDIDATE, |m| {
        if valid_hash_length(cap(m, 1), cap(m, 2)) {
            return;
        }
        c.add(
            cap_start(m, 0),
            end_of(m),
            "hash length does not match its algorithm",
            Cat::Identifier,
            Sev::Error,
        );
    });

    static IDENTIFIER: LazyLock<Regex> = LazyLock::new(|| {
        compile_i(concat!(
            r"(?:№\s*[A-Za-zА-Яа-яЄєІіЇїҐґ0-9]+(?:[-/][A-Za-zА-Яа-яЄєІіЇїҐґ0-9]+)+",
            r"|(?:ЄДРПОУ|РНОКПП|ІПН|ЄРДР)\.?\s*[:№#]?\s*\d{6,20}",
            r"|паспорт\s+[A-Za-zА-Яа-яЄєІіЇїҐґ]{2}\s*\d{6,9}",
            r"|(?:номер\s+картки|картка|картку|картки|карта|карту)\s*\d(?:[ -]?\d){11,18}",
            r"|\b[A-Z]{2}[ -]?\d{2}(?:[ -]?[A-Z0-9]){11,30}\b",
            r"|\b(?:UUID\s*[:=]?\s*)?(?:[0-9A-Fa-f]{8}(?:-[0-9A-Fa-f]{4}){3}-[0-9A-Fa-f]{12}",
            r"|[0-9A-Fa-f]{32})\b",
            r"|\b(?:SHA-?(?:1|224|256|384|512)|SHA3-?(?:256|512)|BLAKE2[bs]|MD5)\s*[:=]?\s*[0-9A-Fa-f]{1,128}\b",
            r"|\b(?:ISBN|ISSN(?:-L)?|VIN|SWIFT|BIC)\b\s*[:№#]?\s*[A-Z0-9 -]{8,32})"
        ))
    });
    each(text, &IDENTIFIER, |m| {
        c.add(
            cap_start(m, 0),
            end_of(m),
            "structured identifier (domain-specific reading may vary)",
            Cat::Identifier,
            Sev::Info,
        );
    });

    static MALFORMED_EMAIL: LazyLock<Regex> =
        LazyLock::new(|| compile(r"\b[A-Za-z0-9._%+\-]+@(?:\s|$|[^\s@.]+(?:\s|$)|[^\s@]*\.\s)"));
    each(text, &MALFORMED_EMAIL, |m| {
        c.add(cap_start(m, 0), end_of(m), "malformed email-like contact", Cat::Web, Sev::Warning);
    });

    static MALFORMED_URL: LazyLock<Regex> =
        LazyLock::new(|| compile_i(r"\bhttps?://(?:\s|$)|\bwww\.(?:\s|$)"));
    each(text, &MALFORMED_URL, |m| {
        c.add(cap_start(m, 0), end_of(m), "malformed URL-like token", Cat::Web, Sev::Warning);
    });

    static POTENTIAL_UNIT: LazyLock<Regex> = LazyLock::new(|| {
        compile(&format!(
            r"(^|[^\d.,:A-Za-z\u{{0080}}-\u{{10FFFF}}])(\d+(?:[.,]\d+)?)\s*([A-Za-z{CYRILLIC}]{{1,6}})(?![A-Za-z{CYRILLIC}])"
        ))
    });
    each(text, &POTENTIAL_UNIT, |m| {
        let original_unit = cap(m, 3);
        let unit = lower_text(original_unit);
        let number = cap(m, 2);
        // An IEEE 802 revision suffix, not a measurement unit.
        if number.starts_with("802.") && number.len() <= 6 {
            return;
        }
        // 2G-6G mobile-network generation.
        if unit == "g" && number.len() == 1 && matches!(number.as_bytes()[0], b'2'..=b'6') {
            return;
        }
        if MEASUREMENTS.contains_key(original_unit)
            || MEASUREMENTS.contains_key(unit.as_str())
            || KNOWN_UNIT_WORDS.contains(&unit)
            || COUNTED_NOUNS.contains_key(unit.as_str())
            || is_ascii_acronym(original_unit)
        {
            return;
        }
        // A probable year followed by ordinary lower-case prose.
        if number.len() == 4 && group_end(m, 2) < cap_start(m, 3) {
            let value = parse_i32(number);
            let first = original_unit.chars().next().unwrap_or('\0');
            if (1000..=2099).contains(&value) && is_uk(first) && !is_upper_uk(first) {
                return;
            }
        }
        // A full lower-case Ukrainian word is usually prose.
        if original_unit.chars().count() > 3
            && original_unit.chars().all(|cp| is_uk(cp) && !is_upper_uk(cp))
        {
            return;
        }
        let start = cap_start(m, 2);
        let end = group_end(m, 3);
        if text[end..].starts_with('/') {
            // The unit may continue past a slash, as in km/h.
            let mut end_of_denominator = end + 1;
            for (letters, cp) in text[end + 1..].chars().enumerate() {
                if letters >= 4 || !(is_latin(cp) || (is_uk(cp) && !is_word_joiner(cp))) {
                    break;
                }
                end_of_denominator += cp.len_utf8();
            }
            let full_unit = format!("{original_unit}{}", &text[end..end_of_denominator]);
            if MEASUREMENTS.contains_key(full_unit.as_str())
                || MEASUREMENTS.contains_key(lower_text(&full_unit).as_str())
            {
                return;
            }
            // Latin c is a common homoglyph in a /с rate.
            if let Some(base) = full_unit.strip_suffix("/c") {
                if MEASUREMENTS.contains_key(format!("{base}/с").as_str()) {
                    return;
                }
            }
        }
        c.add(start, end, "unknown unit or unsupported unit spelling", Cat::Unit, Sev::Warning);
    });

    static FOUR_DIGIT: LazyLock<Regex> =
        LazyLock::new(|| compile(r"(^|[^\d.,])(\d{4})(?!\d|[.,]\d)"));
    static YEAR_CUE: LazyLock<Regex> =
        LazyLock::new(|| compile(r"^\s*(?:рік|року|році|р\.|рр\.|ст\.)"));
    each(text, &FOUR_DIGIT, |m| {
        let n = parse_i32(cap(m, 2));
        if !(1000..=2099).contains(&n) {
            return;
        }
        let start = cap_start(m, 2);
        let end = start + cap(m, 2).len();
        // The reference implementation looks at the next 16 bytes.
        let after_end = (end..=text.len().min(end + 16))
            .rev()
            .find(|&i| text.is_char_boundary(i))
            .unwrap_or(end);
        if YEAR_CUE.is_match(&text[end..after_end]).unwrap_or(false) {
            return;
        }
        c.add(start, end, "four-digit number (year or cardinal?)", Cat::BareNumber, Sev::Warning);
    });

    static CUE_AFTER: LazyLock<Regex> = LazyLock::new(|| {
        compile_i(&format!(r"^\s*(?:{}|%|грн|коп|рік|року|році|тис|млн|млрд|[-–—])", *UNIT_ALT))
    });
    static TRAILING_WORD: LazyLock<Regex> = LazyLock::new(|| compile(r"([А-Яа-яЄєІіЇїҐґ]+)$"));
    static SHORT_NUMBER: LazyLock<Regex> =
        LazyLock::new(|| compile(r"(^|[^\d.,:%\-])(\d{1,4})(?![\d.,:%/\-])"));
    each(text, &SHORT_NUMBER, |m| {
        let start = cap_start(m, 2);
        let digits = cap(m, 2);
        let end = start + digits.len();
        if c.seen.contains(&(start, end)) || (digits.len() > 1 && digits.starts_with('0')) {
            return;
        }
        if let Ok(Some(prev)) = TRAILING_WORD.captures(&text[..start]) {
            let word = lower_text(prev.get(1).map_or("", |g| g.as_str()));
            if GOVERNORS.contains(word.as_str()) {
                return;
            }
        }
        if CUE_AFTER.is_match(&text[end..]).unwrap_or(false) {
            return;
        }
        c.add(
            start,
            end,
            "bare number (case / cardinal-vs-ordinal undetermined)",
            Cat::BareNumber,
            Sev::Warning,
        );
    });

    static AGREEMENT: LazyLock<Regex> = LazyLock::new(|| {
        compile_i(concat!(
            r"(^|[^А-Яа-яЄєІіЇїҐґ-])(близько|понад|перед|між|над|під|при|після|без|від|до|із",
            r"|у|в|на|з)\s+(\d{1,6})\s+([^\s\d,.;:!?()]{3,})"
        ))
    });
    each(text, &AGREEMENT, |m| {
        let noun = lower_text(cap(m, 4));
        let known = COUNTED_NOUNS.contains_key(noun.as_str())
            || MEASUREMENTS.contains_key(cap(m, 4))
            || MEASUREMENTS.contains_key(noun.as_str())
            || COUNTED_OBLIQUE.contains_key(noun.as_str());
        if known {
            return;
        }
        c.add(
            cap_start(m, 3),
            group_end(m, 4),
            "number-noun agreement not verified (noun outside lexicon)",
            Cat::Agreement,
            Sev::Info,
        );
    });

    // Numbers inside a well-formed IP address are not bare numbers or dates.
    static ACCEPTED_IPV4: LazyLock<Regex> = LazyLock::new(|| {
        compile(r"\b(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})(?:/(\d{1,3}))?(?::(\d{1,5}))?\b")
    });
    let mut accepted_networks: Vec<(usize, usize)> = Vec::new();
    each(text, &ACCEPTED_IPV4, |m| {
        let valid = (1..=4).all(|i| parse_i32(cap(m, i)) <= 255)
            && (!matched(m, 5) || parse_i32(cap(m, 5)) <= 32)
            && (!matched(m, 6) || parse_i32(cap(m, 6)) <= 65535);
        if valid {
            accepted_networks.push((cap_start(m, 0), end_of(m)));
        }
    });

    let structured_ranges: Vec<(usize, usize)> = c
        .spans
        .iter()
        .filter(|s| matches!(s.category, Cat::Identifier | Cat::Time))
        .map(|s| (s.start, s.stop))
        .collect();

    let mut spans = c.spans;
    spans.retain(|candidate| {
        let inside_network =
            matches!(candidate.category, Cat::Date | Cat::Fraction | Cat::BareNumber)
                && accepted_networks
                    .iter()
                    .any(|&(start, stop)| start <= candidate.start && stop >= candidate.stop);
        if inside_network {
            return false;
        }
        if !matches!(candidate.category, Cat::BareNumber | Cat::Unit | Cat::ForeignWord | Cat::Time)
        {
            return true;
        }
        // Drop a span that a larger structured span already covers.
        !structured_ranges.iter().any(|&(start, stop)| {
            start <= candidate.start
                && stop >= candidate.stop
                && (start != candidate.start || stop != candidate.stop)
        })
    });
    spans.sort_by_key(|s| (s.start, s.stop));
    spans
}
