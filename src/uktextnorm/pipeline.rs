//! The normalization pipeline: what to protect, which passes to run, and in
//! what order.

use fancy_regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

use super::lexicon;
use super::numbers::number_to_words;
use super::passes::{
    canonicalize_asr, expand_abbreviations, normalize_abbreviations, normalize_addresses,
    normalize_biblical_references, normalize_case_context, normalize_compounds,
    normalize_coordinates, normalize_counted_noun_context, normalize_counted_nouns,
    normalize_currency, normalize_cyrillic_alphanumeric, normalize_dates, normalize_decimals,
    normalize_discourse_dates, normalize_english, normalize_finance, normalize_fractions,
    normalize_homoglyphs, normalize_identifiers, normalize_ip_addresses, normalize_known_acronyms,
    normalize_math, normalize_measurements, normalize_medical, normalize_multipliers,
    normalize_negatives, normalize_number_groups, normalize_ordinal_triggers, normalize_ordinals,
    normalize_overprecise_currency_decimals, normalize_page_ranges, normalize_percent,
    normalize_quarters, normalize_ranges, normalize_regional_currency_aliases,
    normalize_scientific, normalize_section_ranges, normalize_sections, normalize_symbol_currency,
    normalize_symbols, normalize_technical_alphanumeric, normalize_text_with_numbers,
    normalize_text_with_phone_numbers, normalize_time, normalize_typography, normalize_unicode,
    normalize_versions, normalize_web, transliterate_to_cyrillic,
};
use super::re::{cap, compile, compile_i, matched, sub, whole};
use super::text::{
    contains_any, contains_any_token, has_ascii_alpha, has_ascii_digit, has_roman_candidate,
    has_symbol_candidate, lower_text, parse_i32, parse_u64, trim_spaces,
};
use super::validation::{is_valid_date, is_valid_iso_week, valid_hash_length, valid_isbn};
use super::{
    CurrencySymbolPolicy, InputTolerance, NormalizeOptions, NormalizePreset, NumericDateOrder,
    SymbolStyle,
};

/// True when the text mentions any currency symbol, word or code.
fn has_currency_candidate(text: &str) -> bool {
    if lexicon::CURRENCIES
        .iter()
        .any(|entry| !entry.symbol.is_empty() && text.contains(entry.symbol))
    {
        return true;
    }
    let lowered = lower_text(text);
    if contains_any_token(
        &lowered,
        &[
            "грн",
            "долар",
            "євро",
            "фунт",
            "злот",
            "франк",
            "єн",
            "юан",
            "крон",
            "лір",
            "руп",
            "рубл",
            "вон",
            "реал",
            "ранд",
        ],
    ) {
        return true;
    }
    lexicon::CURRENCIES.iter().any(|entry| lowered.contains(&lower_text(entry.code)))
}

/// True when the text mentions a finance ticker as a whole word.
fn has_finance_candidate(text: &str) -> bool {
    if text.contains('₿') {
        return true;
    }
    let lowered = lower_text(text);
    let is_word_byte = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    lexicon::FINANCE_UNITS.iter().any(|entry| {
        let code = lower_text(entry.code);
        let bytes = lowered.as_bytes();
        let mut from = 0;
        while let Some(offset) = lowered[from..].find(&code) {
            let start = from + offset;
            let end = start + code.len();
            let left = start == 0 || !is_word_byte(bytes[start - 1]);
            let right = end == bytes.len() || !is_word_byte(bytes[end]);
            if left && right {
                return true;
            }
            from = end;
        }
        false
    })
}

/// Text that must survive the pipeline unchanged, keyed by a private-use sentinel.
#[derive(Default)]
struct Protected {
    spans: Vec<(String, String)>,
}

impl Protected {
    /// Replaces `value` with a fresh sentinel and remembers it.
    fn protect(&mut self, value: impl Into<String>) -> String {
        let marker = u32::try_from(self.spans.len())
            .ok()
            .and_then(|len| 0xe100_u32.checked_add(len))
            .and_then(char::from_u32)
            .unwrap_or('\u{e0ff}');
        let key = format!("\u{e000}{marker}\u{e001}");
        self.spans.push((key.clone(), value.into()));
        key
    }

    /// Puts every protected span back, newest first.
    fn restore(&self, mut text: String) -> String {
        for (key, value) in self.spans.iter().rev() {
            if text.contains(key.as_str()) {
                text = text.replace(key.as_str(), value);
            }
        }
        text
    }
}

/// True when a dissertation speciality code, not a date, follows.
fn preceded_by_classification_label(prefix: &str) -> bool {
    let tail_start = prefix.len().saturating_sub(96);
    let tail_start =
        (tail_start..=prefix.len()).find(|&i| prefix.is_char_boundary(i)).unwrap_or(prefix.len());
    let lowered = lower_text(&prefix[tail_start..]);
    let lowered = lowered.trim_end();
    // Catalogue entries commonly put the code after "... технічних наук:"
    // rather than after an explicit "спеціальність" label.
    if lowered.ends_with("наук:") && lowered.contains("дис") {
        return true;
    }
    [
        "спеціальністю",
        "спеціальностями",
        "спеціальностей",
        "спеціальності",
        "спеціальність",
        "напряму",
        "код",
        "шифр",
    ]
    .iter()
    .any(|label| lowered.ends_with(label))
}

/// Protects a balanced group starting at each occurrence of `opener`.
///
/// `find_end` receives the text and the offset of the opener and returns the
/// offset just past the group, or `None` to stop scanning.
fn protect_balanced<F>(
    text: &str,
    protected: &mut Protected,
    opener: &str,
    find_end: F,
) -> Option<String>
where
    F: Fn(&str, usize) -> Option<usize>,
{
    let mut out = String::new();
    let mut copied = 0;
    let mut search = 0;
    while let Some(offset) = text[search..].find(opener) {
        let start = search + offset;
        let Some(stop) = find_end(text, start) else { break };
        out.push_str(&text[copied..start]);
        out.push_str(&protected.protect(&text[start..stop]));
        copied = stop;
        search = stop;
    }
    (copied != 0).then(|| {
        out.push_str(&text[copied..]);
        out
    })
}

/// The offset just past a brace-balanced group beginning at `start`.
fn balanced_braces(text: &str, start: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut escaped = false;
    for (i, ch) in text[start..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(start + i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// Wraps up the values the protection pass needs from the options.
struct ProtectionOptions {
    numeric_date_order: NumericDateOrder,
    validate_dates: bool,
}

/// Replaces markup, code, identifiers and impossible values with sentinels so
/// later passes cannot corrupt them.
fn protect_opaque_markup(
    text: &str,
    protected: &mut Protected,
    options: &ProtectionOptions,
) -> String {
    // Citation page markers in Wikipedia extracts look like invalid clock
    // values (".:33–34:39–43"). Remove this metadata before invalid-time
    // protection; otherwise only fragments of the marker are spoken.
    static WIKIPEDIA_PAGE_CITATION: LazyLock<Regex> =
        LazyLock::new(|| compile(r"\.(?::\d+(?:(?:-|–|—)\d+)?)+(?=\s|$)"));
    let mut text = if text.contains(".:") {
        sub(text, &WIKIPEDIA_PAGE_CITATION, |_| ".".to_owned())
    } else {
        text.to_owned()
    };

    // MediaWiki plain-text extracts can retain a TeX serialization after the
    // rendered formula. Treat a balanced `\displaystyle` group like code:
    // partial expansion would corrupt it and is not a spoken rendering.
    if let Some(replaced) = protect_balanced(&text, protected, r"{\displaystyle", balanced_braces) {
        text = replaced;
    }

    // A bracketed run containing IPA letters is a pronunciation, not prose.
    let mut out = String::new();
    let mut copied = 0;
    let mut search = 0;
    while let Some(offset) = text[search..].find('[') {
        let start = search + offset;
        let Some(stop) = text[start + 1..].find(']').map(|i| start + 1 + i) else { break };
        if text[start + 1..stop].contains(['\r', '\n']) {
            break;
        }
        let candidate = &text[start..=stop];
        let has_ipa = candidate.chars().any(|cp| {
            ('\u{250}'..='\u{2ff}').contains(&cp) || ('\u{1d00}'..='\u{1d7f}').contains(&cp)
        });
        if !has_ipa {
            search = stop + 1;
            continue;
        }
        out.push_str(&text[copied..start]);
        out.push_str(&protected.protect(candidate));
        copied = stop + 1;
        search = stop + 1;
    }
    if copied != 0 {
        out.push_str(&text[copied..]);
        text = out;
    }

    static OPAQUE: LazyLock<Regex> = LazyLock::new(|| {
        compile_i(concat!(
            r"(<!--[\s\S]*?-->|```[\s\S]*?```|~~~[\s\S]*?~~~|``(?:[^`\r\n]|`(?!`))*``",
            r"|`[^`\r\n]*`|<(code|pre)\b[^>]*>[\s\S]*?</\2\s*>",
            r"|</?[A-Za-z][A-Za-z0-9:_-]*(?:\s+[^<>]*?)?\s*/?>|<![A-Za-z][^<>]*>",
            r"|&(?:#[0-9]+|#[xX][0-9A-Fa-f]+|[A-Za-z][A-Za-z0-9]+);)"
        ))
    });
    text = sub(&text, &OPAQUE, |m| protected.protect(whole(m)));

    // Scan Markdown destinations by hand so a URL such as `a_(b)` stays opaque
    // all the way to its matching `)`, which a regex cannot balance.
    let mut out = String::new();
    let mut copied = 0;
    let mut search = 0;
    while let Some(offset) = text[search..].find("](") {
        let opener = search + offset;
        let destination = opener + 2;
        let mut depth = 1i32;
        let mut escaped = false;
        let mut pos = destination;
        for (i, ch) in text[destination..].char_indices() {
            pos = destination + i;
            if ch == '\n' || ch == '\r' {
                break;
            }
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
        if depth != 0 {
            break;
        }
        out.push_str(&text[copied..destination]);
        out.push_str(&protected.protect(&text[destination..pos]));
        out.push(')');
        copied = pos + 1;
        search = copied;
    }
    if copied != 0 {
        out.push_str(&text[copied..]);
        text = out;
    }

    static MARKDOWN_REFERENCE: LazyLock<Regex> =
        LazyLock::new(|| compile_i(r"(^|\n)([ \t]{0,3}\[[^\]:\r\n]+\]:[ \t]*)(\S+)"));
    text = sub(&text, &MARKDOWN_REFERENCE, |m| {
        format!("{}{}{}", cap(m, 1), cap(m, 2), protected.protect(cap(m, 3)))
    });

    static MALFORMED_SCIENTIFIC: LazyLock<Regex> =
        LazyLock::new(|| compile(r"(^|[^A-Za-z\d])([+\-]?\d+(?:[.,]\d+)?[eE][+\-]?)(?![+\-]?\d)"));
    static IEEE_REVISION: LazyLock<Regex> = LazyLock::new(|| compile(r"^802\.\d{1,2}[eE]$"));
    text = sub(&text, &MALFORMED_SCIENTIFIC, |m| {
        if IEEE_REVISION.is_match(cap(m, 2)).unwrap_or(false) {
            return whole(m).to_owned();
        }
        format!("{}{}", cap(m, 1), protected.protect(cap(m, 2)))
    });

    static ISBN_CANDIDATE: LazyLock<Regex> = LazyLock::new(|| {
        compile_i(r"\bISBN(?:-1[03])?\s*[:№#]?\s*((?:97[89][ -]?)?[0-9Xx](?:[ -]?[0-9Xx]){8,12})\b")
    });
    text = sub(&text, &ISBN_CANDIDATE, |m| {
        if valid_isbn(cap(m, 1)) {
            whole(m).to_owned()
        } else {
            protected.protect(whole(m))
        }
    });

    static LABELLED_HASH_CANDIDATE: LazyLock<Regex> = LazyLock::new(|| {
        compile_i(
            r"\b((?:SHA-?(?:1|224|256|384|512)|SHA3-?(?:256|512)|BLAKE2[bs]|MD5))\s*[:=]?\s*([0-9A-Fa-f]{1,128})\b",
        )
    });
    text = sub(&text, &LABELLED_HASH_CANDIDATE, |m| {
        if valid_hash_length(cap(m, 1), cap(m, 2)) {
            whole(m).to_owned()
        } else {
            protected.protect(whole(m))
        }
    });

    static ISO_DURATION_CANDIDATE: LazyLock<Regex> =
        LazyLock::new(|| compile_i(r"\bP(?=\d|T(?:\d|[.,]\d))[0-9YMWDTHS.,]+\b"));
    static VALID_ISO_DURATION: LazyLock<Regex> = LazyLock::new(|| {
        compile_i(concat!(
            r"^P(?:(\d+(?:[.,]\d+)?)Y)?(?:(\d+(?:[.,]\d+)?)M)?(?:(\d+(?:[.,]\d+)?)W)?",
            r"(?:(\d+(?:[.,]\d+)?)D)?(?:T(?:(\d+(?:[.,]\d+)?)H)?(?:(\d+(?:[.,]\d+)?)M)?",
            r"(?:(\d+(?:[.,]\d+)?)S)?)?$"
        ))
    });
    text = sub(&text, &ISO_DURATION_CANDIDATE, |m| {
        let token = whole(m);
        let Ok(Some(parsed)) = VALID_ISO_DURATION.captures(token) else {
            return protected.protect(token);
        };
        let has_component = (1..parsed.len()).any(|i| parsed.get(i).is_some());
        if !has_component || token.ends_with(['T', 't']) {
            protected.protect(token)
        } else {
            token.to_owned()
        }
    });

    if text.contains(':') {
        text = normalize_biblical_references(&text);
    }

    static INVALID_CLOCK_CANDIDATE: LazyLock<Regex> =
        LazyLock::new(|| compile(r"(^|[^\d:])(\d{1,3}):(\d{2})(?::(\d{2}))?(?![\d:])"));
    text = sub(&text, &INVALID_CLOCK_CANDIDATE, |m| {
        let hour = parse_i32(cap(m, 2));
        let minute = parse_i32(cap(m, 3));
        let second = matched(m, 4).then(|| parse_i32(cap(m, 4)));
        let invalid = hour > 24
            || (hour == 24 && (minute != 0 || second.is_some_and(|s| s != 0)))
            || minute > 59
            || second.is_some_and(|s| s > 59);
        if !invalid {
            return whole(m).to_owned();
        }
        let value = match second {
            Some(_) => format!("{}:{}:{}", cap(m, 2), cap(m, 3), cap(m, 4)),
            None => format!("{}:{}", cap(m, 2), cap(m, 3)),
        };
        format!("{}{}", cap(m, 1), protected.protect(value))
    });

    static ZONED_CLOCK_CANDIDATE: LazyLock<Regex> = LazyLock::new(|| {
        compile_i(
            r"(\d{1,2}):([0-5]\d)(?::([0-5]\d))?\s*(?:UTC|GMT)\s*([+-])(\d{1,2})(?::?(\d{2}))?",
        )
    });
    text = sub(&text, &ZONED_CLOCK_CANDIDATE, |m| {
        let hour = parse_i32(cap(m, 5));
        let minute = if matched(m, 6) { parse_i32(cap(m, 6)) } else { 0 };
        if hour > 14 || minute > 59 || (hour == 14 && minute != 0) {
            protected.protect(whole(m))
        } else {
            whole(m).to_owned()
        }
    });

    static STANDALONE_TIMEZONE: LazyLock<Regex> =
        LazyLock::new(|| compile_i(r"\b(?:UTC|GMT)\s*[+-](\d{2}):?(\d{2})\b"));
    static BARE_ZONED_CLOCK: LazyLock<Regex> =
        LazyLock::new(|| compile(r"\b\d{1,2}:[0-5]\d(?::[0-5]\d)?\s+[+-](\d{2}):(\d{2})(?!\d)"));
    for re in [&*STANDALONE_TIMEZONE, &*BARE_ZONED_CLOCK] {
        text = sub(&text, re, |m| {
            let hour = parse_i32(cap(m, 1));
            let minute = parse_i32(cap(m, 2));
            if hour > 14 || minute > 59 || (hour == 14 && minute != 0) {
                protected.protect(whole(m))
            } else {
                whole(m).to_owned()
            }
        });
    }

    static IPV4_CANDIDATE: LazyLock<Regex> = LazyLock::new(|| {
        compile(
            r"(^|[^\d.])(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})(?:/(\d{1,3}))?(?::(\d{1,6}))?(?![\d.])",
        )
    });
    text = sub(&text, &IPV4_CANDIDATE, |m| {
        let invalid = (2..=5).any(|i| parse_i32(cap(m, i)) > 255)
            || (matched(m, 6) && parse_i32(cap(m, 6)) > 32)
            || (matched(m, 7) && parse_i32(cap(m, 7)) > 65535);
        if !invalid {
            return whole(m).to_owned();
        }
        let group1 = cap(m, 1);
        format!("{group1}{}", protected.protect(&whole(m)[group1.len()..]))
    });

    static BRACKETED_IPV6_PORT_CANDIDATE: LazyLock<Regex> =
        LazyLock::new(|| compile(r"(^|[^0-9A-Fa-f:])(\[[0-9A-Fa-f:]+\]):(\d{1,6})(?!\d)"));
    text = sub(&text, &BRACKETED_IPV6_PORT_CANDIDATE, |m| {
        if parse_i32(cap(m, 3)) > 65535 {
            let value = format!("{}:{}", cap(m, 2), cap(m, 3));
            format!("{}{}", cap(m, 1), protected.protect(value))
        } else {
            whole(m).to_owned()
        }
    });

    static INVALID_IPV6_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
        compile(
            r"(^|[^0-9A-Fa-f:])((?:[0-9A-Fa-f]{0,4}:){2,7}[0-9A-Fa-f]{0,4}/(\d{1,3}))(?![0-9A-Fa-f:/])",
        )
    });
    text = sub(&text, &INVALID_IPV6_PREFIX, |m| {
        if parse_i32(cap(m, 3)) > 128 {
            format!("{}{}", cap(m, 1), protected.protect(cap(m, 2)))
        } else {
            whole(m).to_owned()
        }
    });

    static GEO_CANDIDATE: LazyLock<Regex> = LazyLock::new(|| {
        compile_i(concat!(
            r"\bgeo\s*:\s*([+\-]?\d{1,3}(?:\.\d+)?)\s*[,;]\s*([+\-]?\d{1,3}(?:\.\d+)?)",
            r"(?:\s*[,;]\s*[+\-]?\d+(?:\.\d+)?)?"
        ))
    });
    text = sub(&text, &GEO_CANDIDATE, |m| {
        let parse = |s: &str| s.parse::<f64>().ok();
        match (parse(cap(m, 1)), parse(cap(m, 2))) {
            (Some(lat), Some(lon)) if lat.abs() <= 90.0 && lon.abs() <= 180.0 => {
                whole(m).to_owned()
            }
            _ => protected.protect(whole(m)),
        }
    });

    static LABELLED_COORDINATE: LazyLock<Regex> = LazyLock::new(|| {
        compile_i(concat!(
            r"(?:lat(?:itude)?|широта)\s*[:=]\s*([+\-]?\d{1,3}(?:[.,]\d+)?)\s*[,; ]+\s*",
            r"(?:lon(?:gitude)?|довгота)\s*[:=]\s*([+\-]?\d{1,3}(?:[.,]\d+)?)"
        ))
    });
    text = sub(&text, &LABELLED_COORDINATE, |m| {
        let parse = |s: &str| s.replace(',', ".").parse::<f64>().ok();
        match (parse(cap(m, 1)), parse(cap(m, 2))) {
            (Some(lat), Some(lon)) if lat.abs() <= 90.0 && lon.abs() <= 180.0 => {
                whole(m).to_owned()
            }
            _ => protected.protect(whole(m)),
        }
    });

    static DMS_COORDINATE: LazyLock<Regex> = LazyLock::new(|| {
        compile_i(
            r#"(\d{1,3})\s*°\s*(?:(\d{1,2})([.,]\d+)?\s*(?:′|')\s*)?(?:(\d{1,2})\s*(?:″|")\s*)?([NSEW])"#,
        )
    });
    text = sub(&text, &DMS_COORDINATE, |m| {
        let marker = lower_text(cap(m, 5));
        let limit = if marker == "n" || marker == "s" { 90 } else { 180 };
        let degrees = parse_i32(cap(m, 1));
        let minutes = if matched(m, 2) { parse_i32(cap(m, 2)) } else { 0 };
        let fractional_minutes =
            matched(m, 3) && cap(m, 3).bytes().any(|b| !matches!(b, b'.' | b',' | b'0'));
        let seconds = if matched(m, 4) { parse_i32(cap(m, 4)) } else { 0 };
        let invalid = degrees > limit
            || minutes > 59
            || seconds > 59
            || (degrees == limit && (minutes != 0 || fractional_minutes || seconds != 0));
        if invalid {
            protected.protect(whole(m))
        } else {
            whole(m).to_owned()
        }
    });

    if options.validate_dates {
        static LOCAL_DATE: LazyLock<Regex> = LazyLock::new(|| {
            compile(concat!(
                r"(^|[^\d./-])(\d{1,2})[./-](\d{1,2})[./-](\d{2}|\d{4})(?!\d)(?![./-]\d)",
                r"(?!x\d)(?!X\d)(?!х\d)(?!Х\d)(?!×\d)"
            ))
        });
        let source = text.clone();
        text = sub(&text, &LOCAL_DATE, |m| {
            let group2_start = m.get(2).map_or(0, |g| g.start());
            if preceded_by_classification_label(&source[..group2_start]) {
                return whole(m).to_owned();
            }
            let first = parse_i32(cap(m, 2));
            let second = parse_i32(cap(m, 3));
            let short_year = parse_i32(cap(m, 4));
            let year = if cap(m, 4).len() == 2 {
                if short_year < 50 {
                    2000 + short_year
                } else {
                    1900 + short_year
                }
            } else {
                short_year
            };
            let month_first = options.numeric_date_order == NumericDateOrder::MonthDayYear
                || (first <= 12
                    && second > 12
                    && (options.numeric_date_order == NumericDateOrder::PreserveAmbiguous
                        || (options.numeric_date_order == NumericDateOrder::DayMonthYear
                            && whole(m).contains('/'))));
            let (day, month) = if month_first { (second, first) } else { (first, second) };
            if is_valid_date(day, month, year) {
                return whole(m).to_owned();
            }
            let group1 = cap(m, 1);
            format!("{group1}{}", protected.protect(&whole(m)[group1.len()..]))
        });

        static ISO_DATE: LazyLock<Regex> =
            LazyLock::new(|| compile(r"\b(\d{4})-(\d{2})-(\d{2})\b"));
        text = sub(&text, &ISO_DATE, |m| {
            if is_valid_date(parse_i32(cap(m, 3)), parse_i32(cap(m, 2)), parse_i32(cap(m, 1))) {
                whole(m).to_owned()
            } else {
                protected.protect(whole(m))
            }
        });

        static ISO_WEEK: LazyLock<Regex> =
            LazyLock::new(|| compile_i(r"\b(\d{4})-W(\d{2})(?:-(\d))?\b"));
        text = sub(&text, &ISO_WEEK, |m| {
            let valid = is_valid_iso_week(parse_i32(cap(m, 1)), parse_i32(cap(m, 2)))
                && (!matched(m, 3) || (1..=7).contains(&parse_i32(cap(m, 3))));
            if valid {
                whole(m).to_owned()
            } else {
                protected.protect(whole(m))
            }
        });

        static ISO_ORDINAL: LazyLock<Regex> = LazyLock::new(|| compile(r"\b(\d{4})-(\d{3})\b"));
        text = sub(&text, &ISO_ORDINAL, |m| {
            let year = parse_i32(cap(m, 1));
            let day = parse_i32(cap(m, 2));
            let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
            if (1..=if leap { 366 } else { 365 }).contains(&day) {
                whole(m).to_owned()
            } else {
                protected.protect(whole(m))
            }
        });
    }

    static IANA_ZONE: LazyLock<Regex> = LazyLock::new(|| {
        compile(r"(\b\d{1,2}:[0-5]\d(?::[0-5]\d)?\s+)([A-Za-z_+-]+/[A-Za-z0-9_+/-]+)\b")
    });
    static KNOWN_ZONES: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
        [
            "europe/kyiv",
            "europe/london",
            "europe/warsaw",
            "america/new_york",
            "america/los_angeles",
            "asia/tokyo",
        ]
        .into_iter()
        .collect()
    });
    sub(&text, &IANA_ZONE, |m| {
        if KNOWN_ZONES.contains(lower_text(cap(m, 2)).as_str()) {
            whole(m).to_owned()
        } else {
            format!("{}{}", cap(m, 1), protected.protect(cap(m, 2)))
        }
    })
}

fn normalize_output_spacing(text: &str) -> String {
    static BEFORE_PUNCTUATION: LazyLock<Regex> = LazyLock::new(|| compile(r"[ \t]+([,.;:!?])"));
    sub(text, &BEFORE_PUNCTUATION, |m| cap(m, 1).to_owned())
}

/// Removes `== heading ==` markers, keeping the heading text.
fn strip_mediawiki_heading_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let trimmed = line.trim_matches(|c| matches!(c, ' ' | '\t' | '\r'));
        let marks = trimmed.bytes().take_while(|&b| b == b'=').count();
        let closing = trimmed.bytes().rev().take_while(|&b| b == b'=').count();
        let content = trimmed
            .get(marks..trimmed.len().saturating_sub(closing))
            .map(|c| c.trim_matches([' ', '\t']));
        match content {
            Some(content)
                if (2..=6).contains(&marks) && marks == closing && !content.is_empty() =>
            {
                out.push_str(content);
            }
            _ => out.push_str(line),
        }
    }
    out
}

/// Protects `$` and `¥`, which several currencies share.
fn protect_ambiguous_currency_symbols(text: &str, protected: &mut Protected) -> String {
    let mut text = text.to_owned();
    for symbol in ['$', '¥'] {
        while let Some(pos) = text.find(symbol) {
            let key = protected.protect(symbol.to_string());
            text.replace_range(pos..pos + symbol.len_utf8(), &key);
        }
    }
    text
}

/// Protects dates where both fields could be a month.
fn protect_ambiguous_numeric_dates(text: &str, protected: &mut Protected) -> String {
    static AMBIGUOUS: LazyLock<Regex> = LazyLock::new(|| {
        compile(r"\b(?:0?[1-9]|1[0-2])[./-](?:0?[1-9]|1[0-2])[./-](?:\d{2}|\d{4})\b")
    });
    sub(text, &AMBIGUOUS, |m| protected.protect(whole(m)))
}

/// Normalizes `text` with the default options.
///
/// ```
/// # use normalize_uk::uktextnorm::normalize;
/// assert_eq!(normalize("5 кг"), "п'ять кілограмів");
/// ```
#[must_use]
pub fn normalize(text: &str) -> String {
    normalize_with(text, &NormalizeOptions::default())
}

/// Normalizes `text` with the options of a named preset.
#[must_use]
pub fn normalize_preset(text: &str, preset: NormalizePreset) -> String {
    normalize_with(text, &NormalizeOptions::preset(preset))
}

/// Normalizes `text` with explicit options.
pub fn normalize_with(text: &str, options: &NormalizeOptions) -> String {
    let mut protected = Protected::default();
    let protection = ProtectionOptions {
        numeric_date_order: options.numeric_date_order,
        validate_dates: options.validate_dates,
    };
    let mut text = protect_opaque_markup(text, &mut protected, &protection);
    if options.currency_symbol_policy == CurrencySymbolPolicy::PreserveAmbiguous {
        text = protect_ambiguous_currency_symbols(&text, &mut protected);
    }
    if options.numeric_date_order == NumericDateOrder::PreserveAmbiguous {
        text = protect_ambiguous_numeric_dates(&text, &mut protected);
    }

    text = normalize_unicode(&text, options.quote_style);
    text = normalize_typography(&text);

    // ASR tolerance: fold distorted Cyrillic tokens back to a canonical surface
    // form before any rule pass runs, so the acronym/brand passes downstream see
    // clean input (`пдв` -> `ПДВ`, `ватсап` -> `вотсап`). No-op under Strict.
    if options.input_tolerance == InputTolerance::Asr {
        text = canonicalize_asr(&text, options.input_tolerance, &options.asr_vocabulary);
    }

    // Isolated mathematical variables must not pass through Latin/Cyrillic
    // homoglyph repair (ρh would otherwise become the unreadable ρг).
    for (from, to) in [
        ("ρh", " ро аш"),
        ("з α =", "з альфою, що дорівнює"),
        ("З α =", "З альфою, що дорівнює"),
        ("α =", "альфа ="),
    ] {
        if text.contains(from) {
            text = text.replace(from, to);
        }
    }
    if text.contains('$') || text.contains('¥') {
        text = normalize_regional_currency_aliases(&text);
    }
    if text.contains("==") {
        text = strip_mediawiki_heading_markup(&text);
    }
    if options.normalize_network_addresses && contains_any(&text, ".:-") {
        text = normalize_ip_addresses(&text);
    }
    if has_ascii_digit(&text) {
        text = normalize_page_ranges(&text, options.range_style);
    }
    if options.normalize_english_words && has_ascii_digit(&text) && has_ascii_alpha(&text) {
        // Progressive-scan resolution suffixes must be read before homoglyph
        // repair turns the Latin p in "1080p-якістю" into Cyrillic р.
        static QUALITY_RESOLUTION: LazyLock<Regex> = LazyLock::new(|| {
            compile(
                r"(^|[^A-Za-z0-9А-Яа-яЄєІіЇїҐґ])(480|576|720|1080|1440|2160|4320)[pP]-(якістю|якість)",
            )
        });
        text = sub(&text, &QUALITY_RESOLUTION, |m| {
            format!("{}{} {} пі", cap(m, 1), cap(m, 3), number_to_words(parse_u64(cap(m, 2))))
        });
        static PROGRESSIVE_RESOLUTION: LazyLock<Regex> = LazyLock::new(|| {
            compile(
                r"(^|[^A-Za-z0-9А-Яа-яЄєІіЇїҐґ])(480|576|720|1080|1440|2160|4320)[pP](?![A-Za-z0-9])",
            )
        });
        text = sub(&text, &PROGRESSIVE_RESOLUTION, |m| {
            format!("{}{} пі", cap(m, 1), number_to_words(parse_u64(cap(m, 2))))
        });
    }
    if options.repair_homoglyphs && has_ascii_alpha(&text) {
        text = normalize_homoglyphs(&text);
    }
    let web_candidate = contains_any(&text, "@#")
        || (has_ascii_alpha(&text) && text.contains('.'))
        || contains_any_token(
            &text,
            &[
                "http://", "https://", "ftp://", "www.", ".com", ".ua", ".org", ".net", ".info",
                ".io", ".edu", ".gov", ".укр",
            ],
        );
    if web_candidate {
        text = normalize_web(&text);
    }
    if contains_any_token(&text, &["кв.", "квартал"]) || has_roman_candidate(&text) {
        text = normalize_quarters(&text);
    }
    if text.contains('.') {
        text = normalize_addresses(&text);
    }
    text = normalize_abbreviations(&text);
    if has_finance_candidate(&text) {
        text = normalize_finance(&text, false);
    }
    if has_ascii_digit(&text) {
        static GROUPED_CURRENCY: LazyLock<Regex> = LazyLock::new(|| {
            compile_i(&format!(
                r"(?:{})\s*[1-9]\d{{0,2}}(?:(?:,\d{{3}})+\.\d{{1,4}}|(?:\.\d{{3}})+,\d{{1,4}})",
                *super::patterns::CURRENCY_TOKEN_ALT
            ))
        });
        if GROUPED_CURRENCY.is_match(&text).unwrap_or(false) {
            text = normalize_currency(&text);
        }
        text = normalize_number_groups(&text, options.parse_thousand_separators);
        text = normalize_identifiers(&text);
        text = normalize_cyrillic_alphanumeric(&text);
        text = normalize_text_with_phone_numbers(&text, options.phone_style);
        text = normalize_scientific(&text, options.range_style);
        text = normalize_dates(
            &text,
            options.date_style,
            options.validate_dates,
            options.range_style,
            options.numeric_date_order,
        );
        text = normalize_section_ranges(&text, options.range_style);
        text = normalize_ranges(&text, options.range_style);
        text = normalize_discourse_dates(&text);
        text = normalize_coordinates(&text);
        let medical_candidate = contains_any(&text, "/°№℃℉\u{212a}")
            || contains_any_token(
                &text,
                &["мм рт", "раз", "тиск", "градус", "град.", "K", "К", "кельвін"],
            );
        if medical_candidate {
            text = normalize_medical(&text);
        }
        text = normalize_counted_noun_context(&text);
        if text.contains('%') {
            text = normalize_percent(&text);
        }
        if contains_any_token(&text, &["тис", "млн", "млрд", "трлн"]) {
            text = normalize_multipliers(&text, true);
        }
        text = normalize_case_context(&text);
        if text.contains('.') || has_roman_candidate(&text) {
            text = normalize_sections(&text);
        }
        if contains_any(&text, ":-–—") {
            text = normalize_time(&text, options.colon_style);
        }
        text = normalize_counted_nouns(&text);
        text = normalize_ordinal_triggers(&text);
        if contains_any(&text, "-–—") {
            text = normalize_compounds(&text);
        }
        text = normalize_ordinals(&text);
        if contains_any(&text, "/½⅓⅔¼¾⅕⅖⅗⅘⅙⅚⅐⅛⅜⅝⅞⅑⅒") {
            text = normalize_fractions(&text);
        }
        if has_currency_candidate(&text) {
            text = normalize_symbol_currency(&text);
        }
        if contains_any_token(&text, &["тис", "млн", "млрд", "трлн"]) {
            text = normalize_multipliers(&text, false);
        }
        text = normalize_measurements(&text);
    } else if has_roman_candidate(&text)
        || text.contains("ХХ")
        || text.contains("ХІ")
        || text.contains("ІХ")
    {
        text = normalize_ordinals(&text);
    }
    if !has_ascii_digit(&text) && contains_any(&text, "½⅓⅔¼¾⅕⅖⅗⅘⅙⅚⅐⅛⅜⅝⅞⅑⅒")
    {
        text = normalize_fractions(&text);
    }
    if has_ascii_digit(&text) && has_currency_candidate(&text) {
        text = normalize_currency(&text);
        text = normalize_overprecise_currency_decimals(&text);
    }
    text = normalize_finance(&text, true);
    if options.expand_known_acronyms {
        text = normalize_known_acronyms(&text);
    }
    if options.spell_unknown_acronyms {
        text = expand_abbreviations(&text);
    }
    if options.symbol_style == SymbolStyle::Expand {
        if text.contains('+') && has_ascii_digit(&text) {
            text = normalize_math(&text);
        }
        if has_symbol_candidate(&text) {
            text = normalize_symbols(&text);
        }
    }
    if has_ascii_digit(&text) {
        if text.contains('.') {
            text = normalize_versions(&text);
        }
        if text.contains(',') || text.contains('.') {
            text = normalize_decimals(&text);
        }
        text = normalize_text_with_phone_numbers(&text, options.phone_style);
        if text.contains('-') || text.contains('−') {
            text = normalize_negatives(&text);
        }
        text = normalize_text_with_numbers(&text);
    }
    if has_ascii_alpha(&text) && has_ascii_digit(&text) {
        text = normalize_technical_alphanumeric(&text);
    }
    if options.normalize_english_words && has_ascii_alpha(&text) {
        text = normalize_english(&text, &options.vocabulary, options.input_tolerance);
    }
    if options.transliterate_latin {
        text = transliterate_to_cyrillic(&text);
    }
    protected.restore(normalize_output_spacing(&trim_spaces(&text)))
}
