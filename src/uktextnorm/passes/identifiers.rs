//! Network addresses, geographic coordinates and structured identifiers.

use fancy_regex::Regex;
use once_cell::sync::Lazy;

use crate::uktextnorm::lexicon::Forms;
use crate::uktextnorm::morphology::plural;
use crate::uktextnorm::numbers::{
    decimal_to_words, decimal_to_words_or_digits, number_digits_or_words, number_to_words,
    number_to_words_case_str, number_to_words_digit_by_digit, number_words_for_gender,
};
use crate::uktextnorm::re::{cap, compile, compile_i, matched, sub, sub_ctx, whole};
use crate::uktextnorm::readers::{read_structured_identifier, spell_identifier_letters};
use crate::uktextnorm::text::{is_latin, is_uk, lower_text, parse_i32, parse_u64};

const DEGREE_FORMS: Forms =
    Forms { one: "градус", few: "градуси", many: "градусів" };
const MINUTE_FORMS: Forms =
    Forms { one: "хвилина", few: "хвилини", many: "хвилин" };
const SECOND_FORMS: Forms =
    Forms { one: "секунда", few: "секунди", many: "секунд" };

fn join_with(parts: &[String], separator: &str) -> String {
    parts.join(separator)
}

/// Reads one IPv6 group, digit runs as digits and letters spelled out.
fn read_ipv6_group(group: &str) -> String {
    if group.is_empty() {
        return "порожня група".to_owned();
    }
    let mut parts: Vec<String> = Vec::new();
    let mut digits = String::new();
    for ch in group.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            continue;
        }
        if !digits.is_empty() {
            parts.push(number_to_words_digit_by_digit(&digits));
            digits.clear();
        }
        parts.push(spell_identifier_letters(&ch.to_ascii_uppercase().to_string()));
    }
    if !digits.is_empty() {
        parts.push(number_to_words_digit_by_digit(&digits));
    }
    join_with(&parts, " ")
}

/// True when every group is a valid hex quartet and the compression is well formed.
fn valid_ipv6(value: &str) -> bool {
    // At most one "::" compression is allowed.
    if value.find("::") != value.rfind("::") {
        return false;
    }
    let compressed = value.contains("::");
    let groups = value
        .split(':')
        .filter(|g| !g.is_empty())
        .map(|g| (g.len() <= 4 && g.bytes().all(|b| b.is_ascii_hexdigit())).then_some(()))
        .collect::<Option<Vec<_>>>();
    let Some(groups) = groups else { return false };
    if compressed {
        groups.len() < 8
    } else {
        groups.len() == 8
    }
}

fn read_ipv6_address(value: &str) -> Option<String> {
    if !valid_ipv6(value) {
        return None;
    }
    let mut groups: Vec<String> = Vec::new();
    let bytes = value.as_bytes();
    let mut start = 0;
    while start < bytes.len() {
        if bytes[start..].starts_with(b"::") {
            groups.push("скорочення нулів".to_owned());
            start += 2;
            continue;
        }
        if bytes[start] == b':' {
            start += 1;
            continue;
        }
        let end = value[start..].find(':').map_or(value.len(), |i| start + i);
        groups.push(read_ipv6_group(&value[start..end]));
        start = end;
    }
    Some(format!("ай пі версії шість {}", join_with(&groups, " двокрапка ")))
}

fn read_ipv4_address(groups: [&str; 4]) -> Option<String> {
    let mut parts = vec!["ай пі".to_owned()];
    for group in groups {
        if group.is_empty() || group.len() > 3 || !group.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let octet: u64 = group.parse().ok()?;
        if octet > 255 {
            return None;
        }
        parts.push(number_to_words(octet));
    }
    Some(join_with(&parts, " "))
}

/// True when the token before the match labels a version rather than an address.
fn preceded_by_version_label(prefix: &str) -> bool {
    let trimmed = prefix.trim_end_matches(|c: char| c.is_ascii() && c <= ' ');
    let start = trimmed
        .rfind(|c: char| (c.is_ascii() && c <= ' ') || matches!(c, '(' | '[' | ':' | '='))
        .map_or(0, |i| i + trimmed[i..].chars().next().map_or(0, char::len_utf8));
    matches!(lower_text(&trimmed[start..]).as_str(), "версія" | "версії" | "version" | "ver" | "v")
}

/// Reads MAC addresses, IPv4 and IPv6 addresses with optional prefix and port.
pub(crate) fn normalize_ip_addresses(text: &str) -> String {
    static CISCO_MAC: Lazy<Regex> = Lazy::new(|| {
        compile(concat!(
            r"(^|[^0-9A-Fa-f])([0-9A-Fa-f]{2})([0-9A-Fa-f]{2})\.([0-9A-Fa-f]{2})",
            r"([0-9A-Fa-f]{2})\.([0-9A-Fa-f]{2})([0-9A-Fa-f]{2})(?![0-9A-Fa-f])"
        ))
    });
    static MAC: Lazy<Regex> = Lazy::new(|| {
        compile(concat!(
            r"(^|[^0-9A-Fa-f])([0-9A-Fa-f]{2})[:-]([0-9A-Fa-f]{2})[:-]([0-9A-Fa-f]{2})[:-]",
            r"([0-9A-Fa-f]{2})[:-]([0-9A-Fa-f]{2})[:-]([0-9A-Fa-f]{2})(?![0-9A-Fa-f])"
        ))
    });
    static IPV4: Lazy<Regex> = Lazy::new(|| {
        compile(concat!(
            r"(^|[^\d.])(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})",
            r"(?:/(\d{1,3}))?(?::(\d{1,5}))?(?![\d:/]|\.\d)"
        ))
    });
    static BRACKETED_IPV6_PORT: Lazy<Regex> =
        Lazy::new(|| compile(r"(^|[^0-9A-Fa-f:])\[([0-9A-Fa-f:]+)\]:(\d{1,5})(?!\d)"));
    static BRACKETED_IPV6: Lazy<Regex> =
        Lazy::new(|| compile(r"(^|[^0-9A-Fa-f:])\[([0-9A-Fa-f:]+)\](?!:)"));
    static IPV6: Lazy<Regex> = Lazy::new(|| {
        compile(
            r"(^|[^0-9A-Fa-f:])((?:[0-9A-Fa-f]{0,4}:){2,7}[0-9A-Fa-f]{0,4})(?:/(\d{1,3}))?(?![0-9A-Fa-f:/])",
        )
    });

    let read_mac = |m: &fancy_regex::Captures<'_, str>| {
        let groups: Vec<String> = (2..=7).map(|i| read_ipv6_group(cap(m, i))).collect();
        format!("{}мак адреса {}", cap(m, 1), join_with(&groups, " двокрапка "))
    };
    let text = sub(text, &CISCO_MAC, read_mac);
    let text = sub(&text, &MAC, read_mac);

    let text = sub_ctx(&text, &IPV4, |m, prefix| {
        if preceded_by_version_label(prefix) {
            return whole(m).to_owned();
        }
        let Some(address) = read_ipv4_address([cap(m, 2), cap(m, 3), cap(m, 4), cap(m, 5)]) else {
            return whole(m).to_owned();
        };
        let mut out = format!("{}{address}", cap(m, 1));
        if matched(m, 6) {
            let prefix_length = parse_u64(cap(m, 6));
            if prefix_length > 32 {
                return whole(m).to_owned();
            }
            out.push_str(&format!(" префікс {}", number_to_words(prefix_length)));
        }
        if matched(m, 7) {
            let port = parse_u64(cap(m, 7));
            if port > 65535 {
                return whole(m).to_owned();
            }
            out.push_str(&format!(" порт {}", number_to_words(port)));
        }
        out
    });

    let text = sub(&text, &BRACKETED_IPV6_PORT, |m| {
        let Some(address) = read_ipv6_address(cap(m, 2)) else {
            return whole(m).to_owned();
        };
        let port = parse_u64(cap(m, 3));
        if port > 65535 {
            return whole(m).to_owned();
        }
        format!("{}{address} порт {}", cap(m, 1), number_to_words(port))
    });
    let text = sub(&text, &BRACKETED_IPV6, |m| match read_ipv6_address(cap(m, 2)) {
        Some(address) => format!("{}{address}", cap(m, 1)),
        None => whole(m).to_owned(),
    });

    sub(&text, &IPV6, |m| {
        let value = cap(m, 2);
        // Without a hex letter or a "::", this is more likely a time or a ratio.
        let has_hex_letter = value.bytes().any(|b| b.is_ascii_hexdigit() && !b.is_ascii_digit());
        if !has_hex_letter && !value.contains("::") {
            return whole(m).to_owned();
        }
        let Some(address) = read_ipv6_address(value) else { return whole(m).to_owned() };
        let mut out = format!("{}{address}", cap(m, 1));
        if matched(m, 3) {
            let prefix_length = parse_u64(cap(m, 3));
            if prefix_length > 128 {
                return whole(m).to_owned();
            }
            out.push_str(&format!(" префікс {}", number_to_words(prefix_length)));
        }
        out
    })
}

/// Strips a leading sign from a coordinate.
fn coordinate_magnitude(token: &str) -> &str {
    token.strip_prefix('−').or_else(|| token.strip_prefix(['+', '-'])).unwrap_or(token)
}

fn decimal_coordinate(token: &str) -> String {
    let token = coordinate_magnitude(token);
    match token.find(['.', ',']) {
        Some(decimal) => decimal_to_words_or_digits(&token[..decimal], &token[decimal + 1..]),
        None => number_to_words(parse_u64(token)),
    }
}

fn exceeds_coordinate_limit(token: &str, limit: i32) -> bool {
    let magnitude = coordinate_magnitude(token);
    let decimal = magnitude.find(['.', ',']);
    let degrees = parse_i32(&magnitude[..decimal.unwrap_or(magnitude.len())]);
    let nonzero_fraction = decimal.is_some_and(|d| magnitude[d + 1..].bytes().any(|b| b != b'0'));
    degrees > limit || (degrees == limit && nonzero_fraction)
}

/// True for a negative coordinate, ignoring a signed zero.
fn is_negative_coordinate(token: &str) -> bool {
    let Some(rest) = token.strip_prefix('−').or_else(|| token.strip_prefix('-')) else {
        return false;
    };
    rest.bytes().any(|b| (b'1'..=b'9').contains(&b))
}

fn signed_quantity(token: &str) -> String {
    let sign = if token.starts_with('-') || token.starts_with('−') {
        "мінус "
    } else if token.starts_with('+') {
        "плюс "
    } else {
        ""
    };
    format!("{sign}{}", decimal_coordinate(token))
}

/// Names the hemisphere a coordinate marker stands for.
fn coordinate_hemisphere(marker: &str) -> String {
    let marker: String = lower_text(marker).replace(' ', "");
    match marker.as_str() {
        "n" => return "північної широти".to_owned(),
        "s" => return "південної широти".to_owned(),
        "e" => return "східної довготи".to_owned(),
        "w" => return "західної довготи".to_owned(),
        _ => {}
    }
    if marker.starts_with("пн") || marker.contains("північ") {
        "північної широти".to_owned()
    } else if marker.starts_with("пд") || marker.contains("півден") {
        "південної широти".to_owned()
    } else if marker.starts_with("сх") || marker.contains("схід") {
        "східної довготи".to_owned()
    } else if marker.starts_with("зх") || marker.contains("зах") {
        "західної довготи".to_owned()
    } else {
        marker
    }
}

/// Reads geographic coordinates in decimal, degrees-minutes and DMS forms.
pub(crate) fn normalize_coordinates(text: &str) -> String {
    static GEO_URI: Lazy<Regex> = Lazy::new(|| {
        compile_i(concat!(
            r"(^|[^A-Za-zА-Яа-яЄєІіЇїҐґ])((?:geo|координати)\s*[:=]\s*)",
            r"((?:[+\-]|−)?\d{1,2}(?:\.\d+)?)\s*[,;]\s*((?:[+\-]|−)?\d{1,3}(?:\.\d+)?)",
            r"(?:\s*[,;]\s*((?:[+\-]|−)?\d+(?:\.\d+)?))?"
        ))
    });
    static LABELLED_PAIR: Lazy<Regex> = Lazy::new(|| {
        compile_i(concat!(
            r"(^|[^A-Za-z])(?:lat(?:itude)?|широта)\s*[:=]\s*((?:[+\-]|−)?\d{1,2}(?:[.,]\d+)?)",
            r"\s*[,; ]+\s*(?:lon(?:gitude)?|довгота)\s*[:=]\s*((?:[+\-]|−)?\d{1,3}(?:[.,]\d+)?)"
        ))
    });
    static GOVERNED_MARKER: Lazy<Regex> = Lazy::new(|| {
        compile_i(concat!(
            r"(^|[\s(\[{:,;])((?:Від|від|До|до))\s+(\d{1,3})\s*°\s*([NSEW])",
            r"(?:\s+(?:широти|довготи))?(?![A-Za-zА-Яа-яЄєІіЇїҐґ])"
        ))
    });
    static GOVERNED_AXIS: Lazy<Regex> = Lazy::new(|| {
        compile_i(
            r"(^|[\s(\[{:,;])((?:Від|від|До|до))\s+(\d{1,3})\s*°\s*(широти|довготи)(?![А-Яа-яЄєІіЇїҐґ])",
        )
    });
    static DECIMAL_MINUTES: Lazy<Regex> = Lazy::new(|| {
        compile_i(concat!(
            r"(^|[^\d])(\d{1,3})\s*°\s*(\d{1,2})[.,](\d+)\s*(?:′|')\s*",
            r"((?:N|S|E|W)|(?:пн|пд|сх|зх)\.?\s*(?:ш|д)\.?)"
        ))
    });
    static DECIMAL: Lazy<Regex> = Lazy::new(|| {
        compile_i(concat!(
            r"(^|[^\d.,])((?:[+\-])?)(\d{1,3})(?:(?:[.,](\d+)\s*(?:°)?)|(?:°\s*))\s*([NSEW])",
            r"(?:\s+(?:широт[а-яіїєґ]*|довгот[а-яіїєґ]*))?(?![A-Za-zА-Яа-яЄєІіЇїҐґ])"
        ))
    });
    static DMS: Lazy<Regex> = Lazy::new(|| {
        compile_i(concat!(
            r#"(^|[^\d])(\d{1,3})\s*°\s*(\d{1,2})\s*(?:′|')\s*(?:(\d{1,2})\s*(?:″|")\s*)?"#,
            r"((?:N|S|E|W)|(?:пн|пд|сх|зх)\.?\s*(?:ш|д)\.?",
            r"|північн[а-яіїєґ]+\s+широт[а-яіїєґ]+|південн[а-яіїєґ]+\s+широт[а-яіїєґ]+",
            r"|східн[а-яіїєґ]+\s+довгот[а-яіїєґ]+|західн[а-яіїєґ]+\s+довгот[а-яіїєґ]+)"
        ))
    });

    let text = sub(text, &GEO_URI, |m| {
        let latitude = cap(m, 3);
        let longitude = cap(m, 4);
        if exceeds_coordinate_limit(latitude, 90) || exceeds_coordinate_limit(longitude, 180) {
            return whole(m).to_owned();
        }
        let south = is_negative_coordinate(latitude);
        let west = is_negative_coordinate(longitude);
        let mut out = format!(
            "{}географічні координати: {} градуса {} широти, {} градуса {} довготи",
            cap(m, 1),
            decimal_coordinate(latitude),
            if south { "південної" } else { "північної" },
            decimal_coordinate(longitude),
            if west { "західної" } else { "східної" }
        );
        if matched(m, 5) {
            out.push_str(&format!(", висота {} метрів", signed_quantity(cap(m, 5))));
        }
        out
    });

    let text = sub(&text, &LABELLED_PAIR, |m| {
        let latitude = cap(m, 2);
        let longitude = cap(m, 3);
        if exceeds_coordinate_limit(latitude, 90) || exceeds_coordinate_limit(longitude, 180) {
            return whole(m).to_owned();
        }
        format!(
            "{}широта {}{}, довгота {}{}",
            cap(m, 1),
            decimal_coordinate(latitude),
            if is_negative_coordinate(latitude) {
                " південна"
            } else {
                " північна"
            },
            decimal_coordinate(longitude),
            if is_negative_coordinate(longitude) { " західна" } else { " східна" }
        )
    });

    let genitive_degrees = |digits: &str| {
        let value = parse_u64(digits);
        format!(
            "{} {}",
            number_to_words_case_str(value, "gen"),
            if value == 1 { "градуса" } else { "градусів" }
        )
    };

    let text = sub(&text, &GOVERNED_MARKER, |m| {
        let marker = lower_text(cap(m, 4));
        let latitude = marker == "n" || marker == "s";
        let degrees = parse_i32(cap(m, 3));
        if degrees > if latitude { 90 } else { 180 } {
            return whole(m).to_owned();
        }
        format!(
            "{}{} {} {}",
            cap(m, 1),
            cap(m, 2),
            genitive_degrees(cap(m, 3)),
            coordinate_hemisphere(cap(m, 4))
        )
    });

    let text = sub(&text, &GOVERNED_AXIS, |m| {
        let latitude = lower_text(cap(m, 4)).starts_with("широт");
        let degrees = parse_i32(cap(m, 3));
        if degrees > if latitude { 90 } else { 180 } {
            return whole(m).to_owned();
        }
        format!("{}{} {} {}", cap(m, 1), cap(m, 2), genitive_degrees(cap(m, 3)), cap(m, 4))
    });

    let text = sub(&text, &DECIMAL_MINUTES, |m| {
        let degrees = parse_i32(cap(m, 2));
        let minutes = parse_i32(cap(m, 3));
        let marker = lower_text(cap(m, 5));
        let latitude =
            marker == "n" || marker == "s" || marker.starts_with("пн") || marker.starts_with("пд");
        let limit = if latitude { 90 } else { 180 };
        let nonzero_fraction = cap(m, 4).bytes().any(|b| b != b'0');
        if degrees > limit
            || minutes > 59
            || (degrees == limit && (minutes != 0 || nonzero_fraction))
        {
            return whole(m).to_owned();
        }
        let Some(fraction) = decimal_to_words(cap(m, 3), cap(m, 4)) else {
            return whole(m).to_owned();
        };
        format!(
            "{}{} {} {fraction} хвилини {}",
            cap(m, 1),
            number_to_words(degrees as u64),
            plural(degrees as u64, &DEGREE_FORMS),
            coordinate_hemisphere(cap(m, 5))
        )
    });

    // A bare integer followed by N/S/E/W is too ambiguous: N, S and W are also
    // common SI symbols. Require either a degree sign or a decimal value.
    let text = sub(&text, &DECIMAL, |m| {
        let degrees = parse_i32(cap(m, 3));
        let marker = lower_text(cap(m, 5));
        let latitude = marker == "n"
            || marker == "s"
            || marker.starts_with("пн")
            || marker.starts_with("пд")
            || marker.contains("широт");
        let limit = if latitude { 90 } else { 180 };
        let nonzero_fraction = matched(m, 4) && cap(m, 4).bytes().any(|b| b != b'0');
        if degrees > limit || (degrees == limit && nonzero_fraction) {
            return whole(m).to_owned();
        }
        let (value, unit) = if matched(m, 4) {
            (decimal_to_words_or_digits(cap(m, 3), cap(m, 4)), "градуса".to_owned())
        } else {
            (number_to_words(degrees as u64), plural(degrees as u64, &DEGREE_FORMS).to_owned())
        };
        format!("{}{value} {unit} {}", cap(m, 1), coordinate_hemisphere(cap(m, 5)))
    });

    sub(&text, &DMS, |m| {
        let degrees = parse_i32(cap(m, 2));
        let marker = lower_text(cap(m, 5));
        let latitude = marker == "n"
            || marker == "s"
            || marker.starts_with("пн")
            || marker.starts_with("пд")
            || marker.contains("широт");
        if degrees > if latitude { 90 } else { 180 }
            || (matched(m, 3) && parse_i32(cap(m, 3)) > 59)
            || (matched(m, 4) && parse_i32(cap(m, 4)) > 59)
        {
            return whole(m).to_owned();
        }
        let mut parts = vec![
            number_words_for_gender(parse_u64(cap(m, 2)), 'm'),
            plural(degrees as u64, &DEGREE_FORMS).to_owned(),
        ];
        if matched(m, 3) {
            let minutes = parse_u64(cap(m, 3));
            parts.push(number_words_for_gender(minutes, 'f'));
            parts.push(plural(minutes, &MINUTE_FORMS).to_owned());
        }
        if matched(m, 4) {
            let seconds = parse_u64(cap(m, 4));
            parts.push(number_words_for_gender(seconds, 'f'));
            parts.push(plural(seconds, &SECOND_FORMS).to_owned());
        }
        parts.push(coordinate_hemisphere(cap(m, 5)));
        format!("{}{}", cap(m, 1), join_with(&parts, " "))
    })
}

/// Spells each character of a code: digits as digits, letters by name.
fn read_code_characters(value: &str) -> String {
    let parts: Vec<String> = value
        .chars()
        .filter_map(|cp| {
            if cp.is_ascii_digit() {
                Some(number_to_words_digit_by_digit(&cp.to_string()))
            } else if is_latin(cp) || is_uk(cp) {
                Some(spell_identifier_letters(&cp.to_string()))
            } else {
                None
            }
        })
        .collect();
    join_with(&parts, " ")
}

/// Reads the body of a technical standard number.
fn read_standard_body(body: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    let chars: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let cp = chars[i];
        if cp.is_whitespace() {
            i += 1;
            continue;
        }
        if cp.is_ascii_digit() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            let digits: String = chars[start..i].iter().collect();
            parts.push(if digits.len() > 1 && digits.starts_with('0') {
                number_to_words_digit_by_digit(&digits)
            } else {
                number_digits_or_words(&digits)
            });
            continue;
        }
        match cp {
            '.' => parts.push("крапка".to_owned()),
            '-' | '–' | '—' => parts.push("дефіс".to_owned()),
            ':' => parts.push("двокрапка".to_owned()),
            '/' => parts.push("слеш".to_owned()),
            _ if is_latin(cp) || is_uk(cp) => {
                let start = i;
                i += 1;
                while i < chars.len() && (is_latin(chars[i]) || is_uk(chars[i])) {
                    i += 1;
                }
                let letters: String = chars[start..i].iter().collect();
                parts.push(spell_identifier_letters(&letters));
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    join_with(&parts, " ")
}

/// Reads standards, UUIDs, hashes, ISBNs, IBANs, bank cards and plate numbers.
pub(crate) fn normalize_identifiers(text: &str) -> String {
    const STANDARD_LABEL: &str =
        r"(?:(?:ДСТУ|ТУ\s+У)(?:\s+(?:EN\s+)?ISO)?|ДНАОП|ISO(?:\s*/\s*IEC)?|IEC|IEEE|ГОСТ|ДБН)";
    const UKRAINIAN_LETTER: &str =
        r"(?:А|Б|В|Г|Ґ|Д|Е|Є|Ж|З|И|І|Ї|Й|К|Л|М|Н|О|П|Р|С|Т|У|Ф|Х|Ц|Ч|Ш|Щ|Ю|Я)";

    static TECHNICAL_STANDARD: Lazy<Regex> = Lazy::new(|| {
        let standard_atom = format!("(?:[A-Za-z0-9]+|{UKRAINIAN_LETTER})");
        compile_i(&format!(
            concat!(
                r"(^|[\s(\[{{:,;])({})(\s+|-|–|—)",
                r"((?:(?:[A-Za-z]|{})\.)?\d+(?:(?:\s*(?:\.|:|/)\s*|(?:-|–|—)){})*)(?![A-Za-z0-9])"
            ),
            STANDARD_LABEL, UKRAINIAN_LETTER, standard_atom
        ))
    });
    static SLASH: Lazy<Regex> = Lazy::new(|| compile(r"\s*/\s*"));
    // IEEE 802 revisions also occur without the "IEEE" label. They are
    // identifiers, not decimals, ranges or unit-bearing measurements (802.16m).
    static BARE_IEEE_REVISION: Lazy<Regex> = Lazy::new(|| {
        compile_i(
            r"(^|[^A-Za-z0-9.])(802\.\d{1,2}(?:[A-Za-z]{1,3})?(?:(?:-|–|—)\d{4})?)(?![A-Za-z0-9]|\.\d)",
        )
    });
    static UUID: Lazy<Regex> = Lazy::new(|| {
        compile(
            r"\b([0-9A-Fa-f]{8})-([0-9A-Fa-f]{4})-([0-9A-Fa-f]{4})-([0-9A-Fa-f]{4})-([0-9A-Fa-f]{12})\b",
        )
    });
    static COMPACT_UUID: Lazy<Regex> =
        Lazy::new(|| compile_i(r"\bUUID\s*[:=]?\s*([0-9A-Fa-f]{32})\b"));
    static LABELLED_HASH: Lazy<Regex> = Lazy::new(|| {
        compile_i(
            r"\b((?:SHA-?(?:1|224|256|384|512)|SHA3-?(?:256|512)|BLAKE2[bs]|MD5))\s*[:=]?\s*([0-9A-Fa-f]{16,128})\b",
        )
    });
    static ISBN: Lazy<Regex> = Lazy::new(|| {
        compile_i(
            r"\b(ISBN(?:-1[03])?)\s*[:№#]?\s*((?:97[89][ -]?)?[0-9Xx](?:[ -]?[0-9Xx]){8,12})\b",
        )
    });
    static ISSN: Lazy<Regex> =
        Lazy::new(|| compile_i(r"\b(ISSN(?:-L)?)\s*[:№#]?\s*(\d{4})[ -]?(\d{3}[\dXx])\b"));
    static VIN: Lazy<Regex> =
        Lazy::new(|| compile_i(r"\b(VIN)\s*[:№#]?\s*([A-HJ-NPR-Z0-9]{17})\b"));
    static SWIFT: Lazy<Regex> = Lazy::new(|| {
        compile_i(r"\b((?:SWIFT|BIC))\s*[:№#]?\s*([A-Z]{4}[A-Z]{2}[A-Z0-9]{2}(?:[A-Z0-9]{3})?)\b")
    });
    static IBAN: Lazy<Regex> =
        Lazy::new(|| compile_i(r"\bUA\s*(\d{2})(?:\s*(\d{4})){6}\s*(\d{1})\b"));
    static FOREIGN_IBAN: Lazy<Regex> =
        Lazy::new(|| compile_i(r"\b([A-Z]{2})[ -]?(\d{2})((?:[ -]?[A-Z0-9]){11,30})\b"));
    static EDRPOU: Lazy<Regex> =
        Lazy::new(|| compile_i(r"(ЄДРПОУ|ЄДР|код\s+ЄДРПОУ)\s*[:№#]?\s*(\d{8})\b"));
    static TAX_ID: Lazy<Regex> =
        Lazy::new(|| compile_i(r"(РНОКПП|ІПН|податковий\s+номер)\s*[:№#]?\s*(\d{10})\b"));
    static POSTCODE: Lazy<Regex> =
        Lazy::new(|| compile_i(r"(індекс|поштовий\s+індекс)\s*[:№#]?\s*(\d{5})\b"));
    static LEGAL_NUMBER: Lazy<Regex> = Lazy::new(|| {
        compile(concat!(
            r"(^|[^A-Za-zА-Яа-яЄєІіЇїҐґ])((?:справа|Справа|справі|Справі|справу|Справу",
            r"|провадження|Провадження|закон|Закон|закону|Закону|наказ|Наказ|постанова|Постанова",
            r"|розпорядження|Розпорядження|рішення|Рішення|ухвала|Ухвала|договір|Договір",
            r"|контракт|Контракт|рахунок|Рахунок|замовлення|Замовлення|акт|Акт|лист|Лист)",
            r"(?:\s+[^№\s]+)?\s*)№\s*([^\s,.;:!?()]+)"
        ))
    });
    static ERDR: Lazy<Regex> =
        Lazy::new(|| compile_i(r"(^|[^A-Za-zА-Яа-яЄєІіЇїҐґ])(ЄРДР\.?\s*)№?\s*(\d{8,20})(?!\d)"));
    static PASSPORT: Lazy<Regex> = Lazy::new(|| {
        compile_i(r"(^|[^A-Za-zА-Яа-яЄєІіЇїҐґ])(паспорт\s+)([^\s\d]+)\s*(\d{6,9})(?!\d)")
    });
    static BANK_CARD: Lazy<Regex> = Lazy::new(|| {
        compile_i(concat!(
            r"(^|[^A-Za-zА-Яа-яЄєІіЇїҐґ])((?:картка|картку|карта|карту)\s+)(\d{4})",
            r"[\s-]+(?:\*{4}|xxxx|XXXX)[\s-]+(?:\*{4}|xxxx|XXXX)[\s-]+(\d{4})(?!\d)"
        ))
    });
    static FULL_BANK_CARD: Lazy<Regex> = Lazy::new(|| {
        compile_i(concat!(
            r"(^|[^A-Za-zА-Яа-яЄєІіЇїҐґ])((?:картка|картку|карта|карту)\s+)",
            r"(\d{4})[\s-]+(\d{4})[\s-]+(\d{4})[\s-]+(\d{4})(?!\d)"
        ))
    });
    static VARIABLE_BANK_CARD: Lazy<Regex> = Lazy::new(|| {
        compile_i(concat!(
            r"(^|[^A-Za-zА-Яа-яЄєІіЇїҐґ])((?:номер\s+картки|картка|картку|картки|карта|карту)\s+)",
            r"(\d(?:[ -]?\d){11,18})(?!\d)"
        ))
    });
    static PLATE: Lazy<Regex> = Lazy::new(|| {
        compile(concat!(
            r"(^|[\s,.;:!?()])((?:А|В|Е|І|К|М|Н|О|Р|С|Т|Х){2})\s*(\d{4})\s*",
            r"((?:А|В|Е|І|К|М|Н|О|Р|С|Т|Х){2})(?![А-Яа-яЄєІіЇїҐґ])"
        ))
    });

    let digits_only = |s: &str| -> String { s.chars().filter(char::is_ascii_digit).collect() };
    let alnum_only =
        |s: &str| -> String { s.chars().filter(char::is_ascii_alphanumeric).collect() };

    let text = sub(text, &TECHNICAL_STANDARD, |m| {
        let label = sub(cap(m, 2), &SLASH, |_| " слеш ".to_owned());
        let delimiter = cap(m, 3);
        let spoken_delimiter = if delimiter.trim_matches([' ', '\t', '\r', '\n']).is_empty() {
            " "
        } else {
            " дефіс "
        };
        format!("{}{label}{spoken_delimiter}{}", cap(m, 1), read_standard_body(cap(m, 4)))
    });
    let text = sub(&text, &BARE_IEEE_REVISION, |m| {
        format!("{}{}", cap(m, 1), read_standard_body(cap(m, 2)))
    });
    let text = sub(&text, &UUID, |m| {
        let groups: Vec<String> = (1..=5).map(|i| read_code_characters(cap(m, i))).collect();
        format!("ю у ай ді {}", join_with(&groups, " дефіс "))
    });
    let text = sub(&text, &COMPACT_UUID, |m| {
        let value = cap(m, 1);
        let groups: Vec<String> = [(0, 8), (8, 4), (12, 4), (16, 4), (20, 12)]
            .into_iter()
            .map(|(start, length)| read_code_characters(&value[start..start + length]))
            .collect();
        format!("ю у ай ді {}", join_with(&groups, " дефіс "))
    });
    let text = sub(&text, &LABELLED_HASH, |m| {
        format!("{} хеш {}", spell_identifier_letters(cap(m, 1)), read_code_characters(cap(m, 2)))
    });
    let text = sub(&text, &ISBN, |m| {
        format!("ай ес бі ен {}", read_code_characters(&alnum_only(cap(m, 2))))
    });
    let text = sub(&text, &ISSN, |m| {
        let label = if lower_text(cap(m, 1)).contains("-l") {
            "ай ес ес ен ел "
        } else {
            "ай ес ес ен "
        };
        format!(
            "{label}{} {}",
            number_to_words_digit_by_digit(cap(m, 2)),
            read_code_characters(cap(m, 3))
        )
    });
    let text = sub(&text, &VIN, |m| format!("він номер {}", read_code_characters(cap(m, 2))));
    let text = sub(&text, &SWIFT, |m| format!("свіфт код {}", read_code_characters(cap(m, 2))));
    let text = sub(&text, &IBAN, |m| {
        let digits = digits_only(whole(m));
        if digits.len() != 27 {
            return whole(m).to_owned();
        }
        format!(
            "айбан {} {}",
            spell_identifier_letters("UA"),
            number_to_words_digit_by_digit(&digits)
        )
    });
    let text = sub(&text, &FOREIGN_IBAN, |m| {
        format!(
            "айбан {} {} {}",
            spell_identifier_letters(cap(m, 1)),
            number_to_words_digit_by_digit(cap(m, 2)),
            read_code_characters(&alnum_only(cap(m, 3)))
        )
    });
    let text = sub(&text, &EDRPOU, |m| {
        format!(
            "єдиний державний реєстр підприємств та організацій України {}",
            number_to_words_digit_by_digit(cap(m, 2))
        )
    });
    let text = sub(&text, &TAX_ID, |m| {
        format!("{} {}", lower_text(cap(m, 1)), number_to_words_digit_by_digit(cap(m, 2)))
    });
    let text = sub(&text, &POSTCODE, |m| {
        format!("{} {}", lower_text(cap(m, 1)), number_to_words_digit_by_digit(cap(m, 2)))
    });
    let text = sub(&text, &LEGAL_NUMBER, |m| {
        format!("{}{}номер {}", cap(m, 1), cap(m, 2), read_structured_identifier(cap(m, 3)))
    });
    let text = sub(&text, &ERDR, |m| {
        format!(
            "{}єдиний реєстр досудових розслідувань номер {}",
            cap(m, 1),
            number_to_words_digit_by_digit(cap(m, 3))
        )
    });
    let text = sub(&text, &PASSPORT, |m| {
        format!(
            "{}{}{} {}",
            cap(m, 1),
            cap(m, 2),
            spell_identifier_letters(cap(m, 3)),
            number_to_words_digit_by_digit(cap(m, 4))
        )
    });
    let text = sub(&text, &BANK_CARD, |m| {
        format!(
            "{}{}{} зірочки зірочки {}",
            cap(m, 1),
            cap(m, 2),
            number_to_words_digit_by_digit(cap(m, 3)),
            number_to_words_digit_by_digit(cap(m, 4))
        )
    });
    let text = sub(&text, &FULL_BANK_CARD, |m| {
        let groups: Vec<String> =
            (3..=6).map(|i| number_to_words_digit_by_digit(cap(m, i))).collect();
        format!("{}{}{}", cap(m, 1), cap(m, 2), join_with(&groups, " "))
    });
    let text = sub(&text, &VARIABLE_BANK_CARD, |m| {
        format!(
            "{}{}{}",
            cap(m, 1),
            cap(m, 2),
            number_to_words_digit_by_digit(&digits_only(cap(m, 3)))
        )
    });
    sub(&text, &PLATE, |m| {
        format!(
            "{}номерний знак {} {} {}",
            cap(m, 1),
            spell_identifier_letters(cap(m, 2)),
            number_to_words_digit_by_digit(cap(m, 3)),
            spell_identifier_letters(cap(m, 4))
        )
    })
}
