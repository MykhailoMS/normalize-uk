//! Times, fractions, percentages, units of measure, medical notation,
//! scientific notation, decimals, scale words, versions and bare numbers.

use fancy_regex::Regex;
use once_cell::sync::Lazy;
use std::collections::HashMap;

use crate::uktextnorm::lexicon::{Forms, Gender};
use crate::uktextnorm::morphology::{feminine_last, plural, plural_of};
use crate::uktextnorm::numbers::{
    decimal_to_words, decimal_to_words_or_digits, hours_words, minutes_words,
    number_digits_or_words, number_to_words, number_to_words_case_str,
    number_to_words_digit_by_digit, number_words_for_gender, ordinal_words, say_fraction,
    signed_number_words, take_spoken_sign, MINUTE_FORMS, SECOND_FORMS,
};
use crate::uktextnorm::patterns::{MEASUREMENTS_RE, RANGE_PREFIX, SIGNED_NUMBER, UNIT_ALT};
use crate::uktextnorm::re::{
    cap, cap_start, compile, compile_i, matched, sub, sub_around, sub_ctx, whole,
};
use crate::uktextnorm::readers::{
    read_dotted, read_measurement_quantity, spell_identifier_letters, MEASUREMENTS,
};
use crate::uktextnorm::temperature::{
    temperature_quantity_words, temperature_scale, TEMPERATURE_UNIT_PATTERN,
};
use crate::uktextnorm::text::{
    join, lower_text, parse_i32, parse_u64, regex_alternation, split_words, try_parse_u64,
};
use crate::uktextnorm::{ColonStyle, RangeStyle};

use super::ranges::{preceding_word, range_connector};

const PERCENT_FORMS: Forms =
    Forms { one: "відсоток", few: "відсотки", many: "відсотків" };

/// Reads a clock time, or `опівночі` at midnight.
fn clock_words(hour: u64, minute: u64, second: Option<u64>, locative: bool) -> String {
    if (hour == 0 || hour == 24) && minute == 0 && second.unwrap_or(0) == 0 {
        return "опівночі".to_owned();
    }
    let mut out = if locative {
        format!("{} годині", ordinal_words(hour, "loc_f"))
    } else {
        hours_words(hour)
    };
    if minute != 0 {
        out.push_str(&format!(" {}", minutes_words(minute, &MINUTE_FORMS)));
    }
    if let Some(second) = second.filter(|&s| s != 0) {
        out.push_str(&format!(" {}", minutes_words(second, &SECOND_FORMS)));
    }
    out
}

/// True when the match is governed by `о`/`об`, which takes the locative.
fn governed_by_o(prefix: &str, group1: &str) -> bool {
    let word = preceding_word(&format!("{prefix}{group1}"));
    word == "о" || word == "об"
}

/// Reads clock times, time zones and ratios.
pub(crate) fn normalize_time(text: &str, colon_style: ColonStyle) -> String {
    static VARIABLE_RATIO: Lazy<Regex> =
        Lazy::new(|| compile(r"(^|[^A-Za-z\d:])(\d+)\s*:\s*([A-Za-z])(?![A-Za-z\d])"));
    static AM_PM: Lazy<Regex> = Lazy::new(|| {
        compile_i(r"(^|[^\d:])(\d{1,2}):([0-5]\d)(?::([0-5]\d))?\s*(a\.?m\.?|p\.?m\.?)(?![A-Za-z])")
    });
    static ZONED: Lazy<Regex> = Lazy::new(|| {
        compile_i(concat!(
            r"(^|[^\d:])(\d{1,2}):([0-5]\d)(?::([0-5]\d))?\s*(UTC|GMT)",
            r"(?:\s*([+-])\s*(\d{1,2})(?::?([0-5]\d))?)?(?![A-Za-z\d])"
        ))
    });
    static OFFSET_ZONED: Lazy<Regex> = Lazy::new(|| {
        compile(r"(^|[^\d:])(\d{1,2}):([0-5]\d)(?::([0-5]\d))?\s+([+-])(\d{2}):([0-5]\d)(?!\d)")
    });
    static IANA_ZONED: Lazy<Regex> = Lazy::new(|| {
        compile_i(concat!(
            r"(^|[^\d:])(\d{1,2}):([0-5]\d)(?::([0-5]\d))?\s+",
            r"([A-Za-z_+-]+/[A-Za-z0-9_+/-]+)(?![A-Za-z0-9_+/-])"
        ))
    });
    static HMS: Lazy<Regex> =
        Lazy::new(|| compile(r"(^|[^\d:])(\d{1,2}):([0-5]\d):([0-5]\d)(?![\d:])"));
    static O_CLOCK: Lazy<Regex> = Lazy::new(|| {
        compile(
            r"(^|[^А-Яа-яЄєІіЇїҐґ\d])((?:О|о)(?:б)?) (\d{1,2})(?:-|–|—)?(?:й|ій|а|ої)(?![А-Яа-яЄєІіЇїҐґ])",
        )
    });
    static DAY_PERIOD: Lazy<Regex> = Lazy::new(|| {
        compile_i(r"(^|[^\d:])(\d{1,2}):([0-5]\d)\s+(ранку|дня|вечора|ночі)(?![А-Яа-яЄєІіЇїҐґ\d:])")
    });
    static HM: Lazy<Regex> = Lazy::new(|| compile(r"(^|[^\d:])(\d{1,2}):([0-5]\d)(?![\d:])"));
    static RATIO: Lazy<Regex> = Lazy::new(|| compile(r"(^|[^\d:])(\d+):(\d+)(?![\d:])"));

    let text = sub(text, &VARIABLE_RATIO, |m| {
        format!(
            "{}{} до {}",
            cap(m, 1),
            number_to_words(parse_u64(cap(m, 2))),
            spell_identifier_letters(cap(m, 3))
        )
    });

    let text = sub_ctx(&text, &AM_PM, |m, prefix| {
        let hour = parse_u64(cap(m, 2));
        if !(1..=12).contains(&hour) {
            return whole(m).to_owned();
        }
        let period = lower_text(cap(m, 5));
        let pm = period.starts_with('p');
        let clock_hour = if hour == 12 {
            if pm {
                12
            } else {
                0
            }
        } else {
            hour
        };
        let suffix = if !pm && hour == 12 {
            ""
        } else if pm && (hour < 6 || hour == 12) {
            " дня"
        } else if pm {
            " вечора"
        } else if hour < 5 {
            " ночі"
        } else {
            " ранку"
        };
        let second = matched(m, 4).then(|| parse_u64(cap(m, 4)));
        format!(
            "{}{}{suffix}",
            cap(m, 1),
            clock_words(clock_hour, parse_u64(cap(m, 3)), second, governed_by_o(prefix, cap(m, 1)))
        )
    });

    let text = sub(&text, &ZONED, |m| {
        let hour = parse_u64(cap(m, 2));
        let offset = if matched(m, 7) { parse_u64(cap(m, 7)) } else { 0 };
        let offset_minutes = if matched(m, 8) { parse_u64(cap(m, 8)) } else { 0 };
        if hour > 23 || offset > 14 || (offset == 14 && offset_minutes != 0) {
            return whole(m).to_owned();
        }
        let second = matched(m, 4).then(|| parse_u64(cap(m, 4)));
        let mut out = format!(
            "{}{} за всесвітнім координованим часом",
            cap(m, 1),
            clock_words(hour, parse_u64(cap(m, 3)), second, false)
        );
        if matched(m, 6) {
            let sign = if cap(m, 6) == "+" { "плюс " } else { "мінус " };
            out.push_str(&format!(" {sign}{}", hours_words(offset)));
            if offset_minutes != 0 {
                out.push_str(&format!(" {}", minutes_words(offset_minutes, &MINUTE_FORMS)));
            }
        }
        out
    });

    let text = sub(&text, &OFFSET_ZONED, |m| {
        let hour = parse_u64(cap(m, 2));
        let offset = parse_u64(cap(m, 6));
        let offset_minutes = parse_u64(cap(m, 7));
        if hour > 23 || offset > 14 || (offset == 14 && offset_minutes != 0) {
            return whole(m).to_owned();
        }
        let second = matched(m, 4).then(|| parse_u64(cap(m, 4)));
        let sign = if cap(m, 5) == "+" { "плюс " } else { "мінус " };
        let mut out = format!(
            "{}{} за часовим поясом {sign}{}",
            cap(m, 1),
            clock_words(hour, parse_u64(cap(m, 3)), second, false),
            hours_words(offset)
        );
        if offset_minutes != 0 {
            out.push_str(&format!(" {}", minutes_words(offset_minutes, &MINUTE_FORMS)));
        }
        out
    });

    let text = sub(&text, &IANA_ZONED, |m| {
        #[rustfmt::skip]
        static ZONE_NAMES: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
            [
                ("europe/kyiv", "за київським часом"),
                ("europe/london", "за лондонським часом"),
                ("europe/warsaw", "за варшавським часом"),
                ("america/new_york", "за нью-йоркським часом"),
                ("america/los_angeles", "за лос-анджелеським часом"),
                ("asia/tokyo", "за токійським часом"),
            ]
            .into_iter()
            .collect()
        });
        let hour = parse_u64(cap(m, 2));
        let Some(zone) = ZONE_NAMES.get(lower_text(cap(m, 5)).as_str()) else {
            return whole(m).to_owned();
        };
        if hour > 23 {
            return whole(m).to_owned();
        }
        let second = matched(m, 4).then(|| parse_u64(cap(m, 4)));
        format!("{}{} {zone}", cap(m, 1), clock_words(hour, parse_u64(cap(m, 3)), second, false))
    });

    let text = sub_ctx(&text, &HMS, |m, prefix| {
        let hour = parse_u64(cap(m, 2));
        if hour > 23 {
            return whole(m).to_owned();
        }
        format!(
            "{}{}",
            cap(m, 1),
            clock_words(
                hour,
                parse_u64(cap(m, 3)),
                Some(parse_u64(cap(m, 4))),
                governed_by_o(prefix, cap(m, 1))
            )
        )
    });

    let text = sub(&text, &O_CLOCK, |m| {
        format!(
            "{}{} {} година",
            cap(m, 1),
            cap(m, 2),
            ordinal_words(parse_u64(cap(m, 3)), "nom_f")
        )
    });

    let text = sub_ctx(&text, &DAY_PERIOD, |m, prefix| {
        let hour = parse_u64(cap(m, 2));
        if hour > 23 {
            return whole(m).to_owned();
        }
        format!(
            "{}{} {}",
            cap(m, 1),
            clock_words(hour, parse_u64(cap(m, 3)), None, governed_by_o(prefix, cap(m, 1))),
            cap(m, 4)
        )
    });

    let text = sub_ctx(&text, &HM, |m, prefix| {
        if colon_style == ColonStyle::Ratio {
            return whole(m).to_owned();
        }
        let hour = parse_u64(cap(m, 2));
        if hour == 24 && parse_u64(cap(m, 3)) == 0 {
            return format!("{}{}", cap(m, 1), clock_words(hour, 0, None, false));
        }
        if hour > 23 {
            return whole(m).to_owned();
        }
        format!(
            "{}{}",
            cap(m, 1),
            clock_words(hour, parse_u64(cap(m, 3)), None, governed_by_o(prefix, cap(m, 1)))
        )
    });

    sub(&text, &RATIO, |m| {
        if colon_style == ColonStyle::Clock {
            let (Some(hour), Some(minute)) = (try_parse_u64(cap(m, 2)), try_parse_u64(cap(m, 3)))
            else {
                return whole(m).to_owned();
            };
            if hour <= 23 && minute <= 59 {
                return format!("{}{}", cap(m, 1), clock_words(hour, minute, None, false));
            }
            return whole(m).to_owned();
        }
        let (Some(left), Some(right)) = (try_parse_u64(cap(m, 2)), try_parse_u64(cap(m, 3))) else {
            return whole(m).to_owned();
        };
        format!(
            "{}{} до {}",
            cap(m, 1),
            number_to_words(left),
            number_to_words_case_str(right, "gen")
        )
    })
}

/// The vulgar fraction characters and the fraction each stands for.
#[rustfmt::skip]
const VULGAR: [(&str, u64, u64); 18] = [
    ("½", 1, 2), ("⅓", 1, 3), ("⅔", 2, 3), ("¼", 1, 4), ("¾", 3, 4), ("⅕", 1, 5),
    ("⅖", 2, 5), ("⅗", 3, 5), ("⅘", 4, 5), ("⅙", 1, 6), ("⅚", 5, 6), ("⅐", 1, 7),
    ("⅛", 1, 8), ("⅜", 3, 8), ("⅝", 5, 8), ("⅞", 7, 8), ("⅑", 1, 9), ("⅒", 1, 10),
];

/// Reads vulgar fractions, mixed numbers and `n/m` fractions.
pub(crate) fn normalize_fractions(text: &str) -> String {
    let unit_alt: &str = &UNIT_ALT;
    let not_letter = r"(?![A-Za-zА-Яа-яЄєІіЇїҐґ])";
    let mut text = text.to_owned();
    for (symbol, numerator, denominator) in VULGAR {
        if !text.contains(symbol) {
            continue;
        }
        let measured_mixed = compile(&format!(r"(\d+)\s*{symbol}\s*({unit_alt}){not_letter}"));
        text = sub(&text, &measured_mixed, |m| match MEASUREMENTS.get(cap(m, 2)) {
            Some(unit) => format!(
                "{} цілих і {} {}",
                number_words_for_gender(parse_u64(cap(m, 1)), 'f'),
                say_fraction(numerator, denominator),
                unit.decimal
            ),
            None => whole(m).to_owned(),
        });
        let measured_vulgar = compile(&format!(r"(^|[^\d]){symbol}\s*({unit_alt}){not_letter}"));
        text = sub(&text, &measured_vulgar, |m| match MEASUREMENTS.get(cap(m, 2)) {
            Some(unit) => {
                format!("{}{} {}", cap(m, 1), say_fraction(numerator, denominator), unit.decimal)
            }
            None => whole(m).to_owned(),
        });
        let mixed = compile(&format!(r"(\d+)\s*{symbol}"));
        text = sub(&text, &mixed, |m| {
            format!(
                "{} цілих і {}",
                number_words_for_gender(parse_u64(cap(m, 1)), 'f'),
                say_fraction(numerator, denominator)
            )
        });
        text = text.replace(symbol, &format!(" {}", say_fraction(numerator, denominator)));
    }

    let measured_fraction =
        compile(&format!(r"(^|[^\d.,/])([+\-]?)(\d+)/(\d+)\s*({unit_alt}){not_letter}"));
    let text = sub(&text, &measured_fraction, |m| {
        let (Some(numerator), Some(denominator)) =
            (try_parse_u64(cap(m, 3)), try_parse_u64(cap(m, 4)))
        else {
            return whole(m).to_owned();
        };
        let Some(unit) = MEASUREMENTS.get(cap(m, 5)) else { return whole(m).to_owned() };
        if denominator == 0 {
            return whole(m).to_owned();
        }
        let sign = spoken_sign(cap(m, 2));
        format!("{}{sign}{} {}", cap(m, 1), say_fraction(numerator, denominator), unit.decimal)
    });

    static MIXED_NUMBER: Lazy<Regex> =
        Lazy::new(|| compile(r"(^|[^\d.,/])([+\-−]?)(\d+) (\d+)/(\d+)\b"));
    let text = sub(&text, &MIXED_NUMBER, |m| {
        let values = [3, 4, 5].map(|i| try_parse_u64(cap(m, i)));
        let [Some(whole_part), Some(numerator), Some(denominator)] = values else {
            return whole(m).to_owned();
        };
        if denominator == 0 {
            return whole(m).to_owned();
        }
        format!(
            "{}{}{} і {}",
            cap(m, 1),
            spoken_sign(cap(m, 2)),
            number_words_for_gender(whole_part, 'f'),
            say_fraction(numerator, denominator)
        )
    });

    static PLAIN_FRACTION: Lazy<Regex> =
        Lazy::new(|| compile(r"(^|[^\d.,/])([+\-−]?)(\d+)/(\d+)\b"));
    sub(&text, &PLAIN_FRACTION, |m| {
        let (Some(numerator), Some(denominator)) =
            (try_parse_u64(cap(m, 3)), try_parse_u64(cap(m, 4)))
        else {
            return whole(m).to_owned();
        };
        if denominator == 0 {
            return whole(m).to_owned();
        }
        format!("{}{}{}", cap(m, 1), spoken_sign(cap(m, 2)), say_fraction(numerator, denominator))
    })
}

fn spoken_sign(sign: &str) -> &'static str {
    match sign {
        "-" | "−" => "мінус ",
        "+" => "плюс ",
        _ => "",
    }
}

/// Reads percentages, agreeing the noun with the number.
pub(crate) fn normalize_percent(text: &str) -> String {
    static GOVERNED: Lazy<Regex> = Lazy::new(|| {
        compile(concat!(
            r"(^|[^А-Яа-яЄєІіЇїҐґ-])(Близько|близько|Після|після|Менше|менше|Більше|більше",
            r"|Без|без|Від|від|До|до|Із|із)\s+([+\-]?\d+(?:[.,]\d+)?)\s*%"
        ))
    });
    static ADJACENT_SIGNED: Lazy<Regex> = Lazy::new(|| compile(r"([+\-])(\d+(?:[.,]\d+)?)\s*%"));
    static PERCENT: Lazy<Regex> =
        Lazy::new(|| compile(r"(^|[^\d.,+\-])([+\-]?\d+(?:[.,]\d+)?)\s*%"));

    let text = sub(text, &GOVERNED, |m| {
        let token = cap(m, 3);
        let Some(words) = signed_number_words(token, "gen", 'm') else {
            return whole(m).to_owned();
        };
        let unit =
            if token.contains(['.', ',']) { "відсотка" } else { "відсотків" };
        format!("{}{} {words} {unit}", cap(m, 1), cap(m, 2))
    });
    let text = sub(&text, &ADJACENT_SIGNED, |m| {
        let Some(number) = signed_number_words(&format!("{}{}", cap(m, 1), cap(m, 2)), "nom", 'm')
        else {
            return whole(m).to_owned();
        };
        let token = cap(m, 2);
        if token.contains(['.', ',']) {
            return format!(" {number} відсотка");
        }
        match try_parse_u64(token) {
            Some(value) => format!(" {number} {}", plural(value, &PERCENT_FORMS)),
            None => whole(m).to_owned(),
        }
    });
    sub(&text, &PERCENT, |m| {
        let num = cap(m, 2);
        let Some(words) = signed_number_words(num, "nom", 'm') else {
            let mut unsigned = num;
            let sign = take_spoken_sign(&mut unsigned);
            if !unsigned.contains(['.', ',']) {
                return format!(
                    "{}{sign}{} відсотків",
                    cap(m, 1),
                    number_to_words_digit_by_digit(unsigned)
                );
            }
            return whole(m).to_owned();
        };
        let mut unsigned = num;
        take_spoken_sign(&mut unsigned);
        if num.contains(['.', ',']) {
            return format!("{}{words} відсотка", cap(m, 1));
        }
        match try_parse_u64(unsigned) {
            Some(n) => format!("{}{words} {}", cap(m, 1), plural(n, &PERCENT_FORMS)),
            None => whole(m).to_owned(),
        }
    })
}

/// Unit keys that are a single atom, with no `/`, `·`, `-` or space.
static ATOMIC_UNIT_ALT: Lazy<String> = Lazy::new(|| {
    regex_alternation(
        MEASUREMENTS
            .keys()
            .copied()
            .filter(|key| !key.contains(['/', '-', ' ']) && !key.contains('·')),
    )
});

static ATOM_PATTERN: Lazy<String> = Lazy::new(|| format!("(?:{})(?:²|³|2|3)?", *ATOMIC_UNIT_ALT));

/// Reads quantities with units, including tolerances and unit formulas.
pub(crate) fn normalize_measurements(text: &str) -> String {
    let unit_alt: &str = &UNIT_ALT;
    let atom_pattern: &str = &ATOM_PATTERN;
    let number = SIGNED_NUMBER;
    let not_letter = r"(?![A-Za-zА-Яа-яЄєІіЇїҐґ])";

    let tolerance =
        compile(&format!(r"(^|[^\d.,])({number})\s*±\s*({number})\s*({unit_alt}){not_letter}"));
    let text = sub(text, &tolerance, |m| {
        let Some(unit) = MEASUREMENTS.get(cap(m, 4)) else { return whole(m).to_owned() };
        let gender = if unit.gender == Gender::Feminine { 'f' } else { 'm' };
        match signed_number_words(cap(m, 2), "nom", gender) {
            Some(base) => format!(
                "{}{base} плюс мінус {}",
                cap(m, 1),
                read_measurement_quantity(cap(m, 3), unit)
            ),
            None => whole(m).to_owned(),
        }
    });

    let parenthesized_denominator = compile(&format!(
        r"(^|[^\d.,])({number})\s*({atom_pattern})\s*/\s*\(\s*({atom_pattern})\s*(?:·|\*)\s*({atom_pattern})\s*\){not_letter}"
    ));
    let text = sub(&text, &parenthesized_denominator, |m| {
        let units = [3, 4, 5].map(|i| MEASUREMENTS.get(cap(m, i)));
        let [Some(first), Some(second), Some(third)] = units else {
            return whole(m).to_owned();
        };
        format!(
            "{}{} поділити на {} помножити на {}",
            cap(m, 1),
            read_measurement_quantity(cap(m, 2), first),
            second.forms.one,
            third.forms.one
        )
    });

    let formula = compile(&format!(
        r"(^|[^\d.,])([+\-−]?\d+(?:[.,]\d+)?)\s*({atom_pattern}\s*(?:·|\*|/)\s*{atom_pattern}(?:\s*(?:·|\*|/)\s*{atom_pattern})*)(?!\s*/){not_letter}"
    ));
    let atom = compile(atom_pattern);
    let text = sub(&text, &formula, |m| {
        let expression = cap(m, 3);
        if let Some(direct) = MEASUREMENTS.get(expression) {
            return format!("{}{}", cap(m, 1), read_measurement_quantity(cap(m, 2), direct));
        }
        let mut out = cap(m, 1).to_owned();
        let mut previous_end = 0;
        let mut denominator = false;
        let mut first = true;
        let mut pos = 0;
        while let Ok(Some(caps)) = atom.captures_from_pos(expression, pos) {
            let found = caps.get(0).expect("group 0 participates");
            let separator = &expression[previous_end..found.start()];
            let divides = separator.contains('/');
            denominator |= divides;
            let Some(words) = factor_words(found.as_str(), cap(m, 2), first, denominator) else {
                return whole(m).to_owned();
            };
            if !first {
                out.push_str(if denominator && divides {
                    " поділити на "
                } else {
                    " помножити на "
                });
            }
            out.push_str(&words);
            first = false;
            previous_end = found.end();
            pos = found.end().max(found.start() + 1);
        }
        if first {
            whole(m).to_owned()
        } else {
            out
        }
    });

    sub(&text, &MEASUREMENTS_RE, |m| match MEASUREMENTS.get(cap(m, 3)) {
        Some(unit) => {
            format!("{}{}{}", cap(m, 1), read_measurement_quantity(cap(m, 2), unit), cap(m, 4))
        }
        None => whole(m).to_owned(),
    })
}

/// Reads one factor of a unit formula, stripping a squared or cubed exponent.
fn factor_words(factor: &str, quantity: &str, first: bool, denominator: bool) -> Option<String> {
    let mut exponent = "";
    let unit = match MEASUREMENTS.get(factor) {
        Some(unit) => Some(*unit),
        None => {
            let mut found = None;
            for (suffix, words) in
                [("²", " у квадраті"), ("³", " у кубі"), ("2", " у квадраті"), ("3", " у кубі")]
            {
                let Some(base) = factor.strip_suffix(suffix) else { continue };
                if let Some(unit) = MEASUREMENTS.get(base) {
                    found = Some(*unit);
                    exponent = words;
                }
                break;
            }
            found
        }
    }?;
    if first {
        return Some(format!("{}{exponent}", read_measurement_quantity(quantity, unit)));
    }
    let mut name = unit.forms.one;
    if denominator {
        // A unit in the denominator is read in the accusative: "на годину".
        name = match name {
            "секунда" => "секунду",
            "хвилина" => "хвилину",
            "година" => "годину",
            "миля" => "милю",
            "тонна" => "тонну",
            "унція" => "унцію",
            "атмосфера" => "атмосферу",
            other => other,
        };
    }
    Some(format!("{name}{exponent}"))
}

/// Reads concentrations, blood pressure, temperatures, dosing frequency and
/// `№` numbers.
pub(crate) fn normalize_medical(text: &str) -> String {
    static CONCENTRATION: Lazy<Regex> = Lazy::new(|| {
        compile_i(
            r"(^|[^\d.,])(\d+(?:[.,]\d+)?)\s*(мг|мл|г)\s*/\s*(мл|л)(?![A-Za-zА-Яа-яЄєІіЇїҐґ])",
        )
    });
    static PRESSURE: Lazy<Regex> =
        Lazy::new(|| compile_i(r"(^|[^\d.,])(\d{2,3})\s*/\s*(\d{2,3})\s*мм\s*рт\.?\s*ст\.?"));
    static LABELLED_PRESSURE: Lazy<Regex> = Lazy::new(|| {
        compile_i(r"(^|[^А-Яа-яЄєІіЇїҐґ\d])(тиск\s+)(\d{2,3})\s*/\s*(\d{2,3})\s*мм\s*рт\.?\s*ст\.?")
    });
    static FREQUENCY: Lazy<Regex> = Lazy::new(|| {
        compile_i(concat!(
            r"(^|[^А-Яа-яЄєІіЇїҐґ\d])(\d+)\s*(?:р\.|раз(?:и|ів)?)",
            r"(\s+на\s+(?:день|добу|тиждень|місяць))(?![А-Яа-яЄєІіЇїҐґ])"
        ))
    });
    static NUMBER_SIGN: Lazy<Regex> =
        Lazy::new(|| compile(r"(^|[^А-Яа-яЄєІіЇїҐґ\d])№\s*(\d{1,4})(?![\d/])"));

    let number = SIGNED_NUMBER;
    let temperature_unit: &str = &TEMPERATURE_UNIT_PATTERN;
    let prefix = RANGE_PREFIX;
    let not_letter = r"(?![A-Za-zА-Яа-яЄєІіЇїҐґ])";

    let text = sub(text, &CONCENTRATION, |m| {
        let from = lower_text(cap(m, 3));
        let to = lower_text(cap(m, 4));
        let (Some(from), Some(to)) =
            (MEASUREMENTS.get(from.as_str()), MEASUREMENTS.get(to.as_str()))
        else {
            return whole(m).to_owned();
        };
        format!("{}{} на {}", cap(m, 1), read_measurement_quantity(cap(m, 2), from), to.forms.one)
    });
    let text = sub(&text, &LABELLED_PRESSURE, |m| {
        format!(
            "{}{}{} на {} міліметрів ртутного стовпа",
            cap(m, 1),
            cap(m, 2),
            number_to_words(parse_u64(cap(m, 3))),
            number_to_words(parse_u64(cap(m, 4)))
        )
    });
    let text = sub(&text, &PRESSURE, |m| {
        format!(
            "{}{} на {} міліметрів ртутного стовпа",
            cap(m, 1),
            number_to_words(parse_u64(cap(m, 2))),
            number_to_words(parse_u64(cap(m, 3)))
        )
    });

    let temperature_tolerance = compile(&format!(
        r"{prefix}({number})\s*±\s*({number})\s*({temperature_unit}){not_letter}"
    ));
    let text = sub(&text, &temperature_tolerance, |m| {
        let Some(scale) = temperature_scale(cap(m, 4)) else { return whole(m).to_owned() };
        let base = signed_number_words(cap(m, 2), "nom", 'm');
        let tolerance = temperature_quantity_words(cap(m, 3), &scale, "nom");
        match (base, tolerance) {
            (Some(base), Some(tolerance)) => {
                format!("{}{base} плюс мінус {tolerance}", cap(m, 1))
            }
            _ => whole(m).to_owned(),
        }
    });

    let governed_temperature = compile(&format!(
        concat!(
            r"(^|[^А-Яа-яЄєІіЇїҐґ-])(Близько|близько|Після|після|Менше|менше|Більше|більше",
            r"|Без|без|Від|від|До|до|Із|із)\s+({})\s*({}){}"
        ),
        number, temperature_unit, not_letter
    ));
    let text = sub(&text, &governed_temperature, |m| {
        let Some(scale) = temperature_scale(cap(m, 4)) else { return whole(m).to_owned() };
        match temperature_quantity_words(cap(m, 3), &scale, "gen") {
            Some(words) => format!("{}{} {words}", cap(m, 1), cap(m, 2)),
            None => whole(m).to_owned(),
        }
    });

    let temperature = compile(&format!(r"{prefix}({number})\s*({temperature_unit}){not_letter}"));
    let text = sub(&text, &temperature, |m| {
        let Some(scale) = temperature_scale(cap(m, 3)) else { return whole(m).to_owned() };
        match temperature_quantity_words(cap(m, 2), &scale, "nom") {
            Some(words) => format!("{}{words}", cap(m, 1)),
            None => whole(m).to_owned(),
        }
    });

    let text = sub(&text, &FREQUENCY, |m| {
        let n = parse_u64(cap(m, 2));
        format!(
            "{}{} {}{}",
            cap(m, 1),
            number_to_words(n),
            plural(n, &Forms { one: "раз", few: "рази", many: "разів" }),
            cap(m, 3)
        )
    });
    sub(&text, &NUMBER_SIGN, |m| {
        format!("{}номер {}", cap(m, 1), number_to_words(parse_u64(cap(m, 2))))
    })
}

/// Rewrites superscript digits as a `^` exponent.
fn lower_superscripts(text: &str) -> String {
    // The superscript digits are not contiguous: ¹²³ sit in Latin-1 Supplement
    // while the rest are in Superscripts and Subscripts.
    #[rustfmt::skip]
    const SUPERSCRIPTS: [(char, char); 12] = [
        ('\u{2070}', '0'), ('\u{00b9}', '1'), ('\u{00b2}', '2'), ('\u{00b3}', '3'),
        ('\u{2074}', '4'), ('\u{2075}', '5'), ('\u{2076}', '6'), ('\u{2077}', '7'),
        ('\u{2078}', '8'), ('\u{2079}', '9'), ('\u{207b}', '-'), ('\u{207a}', '+'),
    ];
    let digit = |cp: char| SUPERSCRIPTS.iter().find(|&&(from, _)| from == cp).map(|&(_, to)| to);
    let mut out = String::with_capacity(text.len());
    let mut in_exponent = false;
    for cp in text.chars() {
        match digit(cp) {
            Some(plain) => {
                if !in_exponent {
                    // A superscript only starts an exponent right after a digit.
                    if !out.ends_with(|c: char| c.is_ascii_digit()) {
                        out.push(cp);
                        continue;
                    }
                    out.push('^');
                    in_exponent = true;
                }
                out.push(plain);
            }
            None => {
                in_exponent = false;
                out.push(cp);
            }
        }
    }
    out
}

fn exponent_words(token: &str) -> Option<String> {
    let token = token.strip_prefix('−').map_or(token, |rest| rest);
    let negative = token.starts_with('-');
    let (sign, token) = if negative || token.starts_with('+') {
        (if negative { "мінус " } else { "плюс " }, &token[1..])
    } else {
        ("", token)
    };
    try_parse_u64(token).map(|value| format!("{sign}{}", number_to_words(value)))
}

/// The exponent token as written, with a Unicode minus turned into a hyphen.
fn exponent_token(token: &str) -> String {
    match token.strip_prefix('−') {
        Some(rest) => format!("-{rest}"),
        None => token.to_owned(),
    }
}

fn scientific_words(base: &str, exponent: &str) -> Option<String> {
    let base_words = signed_number_words(base, "nom", 'm')?;
    let exponent_text = exponent_words(&exponent_token(exponent))?;
    Some(format!("{base_words} помножити на десять у степені {exponent_text}"))
}

/// The unit after a scientific quantity, agreeing with the mantissa.
fn scientific_unit(quantity: &str, unit: &str) -> String {
    if unit.is_empty() {
        return String::new();
    }
    let Some(measurement) = MEASUREMENTS.get(unit) else { return String::new() };
    let quantity = quantity.strip_prefix(['+', '-']).unwrap_or(quantity);
    if quantity.contains(['.', ',']) {
        return format!(" {}", measurement.decimal);
    }
    match try_parse_u64(quantity) {
        Some(value) => format!(" {}", plural(value, &measurement.forms)),
        None => String::new(),
    }
}

/// Reads scientific notation, powers of ten and exponents.
pub(crate) fn normalize_scientific(text: &str, range_style: RangeStyle) -> String {
    let text = lower_superscripts(text);
    let unit_alt: &str = &UNIT_ALT;
    let unit_suffix = format!(r"(?:\s*({unit_alt}))?(?![A-Za-zА-Яа-яЄєІіЇїҐґ])");
    let optional_unit = format!(r"(?:\s*({unit_alt}))?");

    static INVERSE_CELSIUS_POWER: Lazy<Regex> = Lazy::new(|| {
        compile(
            r"(^|[^\d])([+-]?\d+(?:[.,]\d+)?)\s*(?:×|·|x|X|\*)\s*10−(\d+)\s*°[CС](?:−|-)(\d+)(?!\d)",
        )
    });
    let text = sub(&text, &INVERSE_CELSIUS_POWER, |m| {
        let Some(words) = scientific_words(cap(m, 2), &format!("-{}", cap(m, 3))) else {
            return whole(m).to_owned();
        };
        let inverse = parse_u64(cap(m, 4));
        let power = if inverse == 1 {
            String::new()
        } else {
            format!(" у степені {}", number_to_words(inverse))
        };
        format!("{}{words} на градус Цельсія{power}", cap(m, 1))
    });

    let times_ten = compile(&format!(
        r"(^|[^A-Za-zА-Яа-яЄєІіЇїҐґ\d.,])([+-]?\d+(?:[.,]\d+)?)\s*(?:×|·|x|X|\*)\s*10\s*\^\s*([+-]?\d+)(?!\d){unit_suffix}"
    ));
    let text = sub(&text, &times_ten, |m| match scientific_words(cap(m, 2), cap(m, 3)) {
        Some(words) => {
            format!("{}{words}{}", cap(m, 1), scientific_unit(cap(m, 2), cap(m, 4)))
        }
        None => whole(m).to_owned(),
    });

    let times_ten_plain = compile(&format!(
        r"(^|[^A-Za-zА-Яа-яЄєІіЇїҐґ\d.,])([+-]?\d+(?:[.,]\d+)?)\s*(?:×|·|x|X|\*)\s*10\s*((?:\+|-|−)\d+)(?!\d){optional_unit}"
    ));
    let text = sub(&text, &times_ten_plain, |m| match scientific_words(cap(m, 2), cap(m, 3)) {
        Some(words) => {
            format!("{}{words}{}", cap(m, 1), scientific_unit(cap(m, 2), cap(m, 4)))
        }
        None => whole(m).to_owned(),
    });

    static E_NOTATION: Lazy<Regex> = Lazy::new(|| {
        compile(
            r"(^|[^A-Za-zА-Яа-яЄєІіЇїҐґ\d.,])([+-]?\d+(?:[.,]\d+)?)[eE]([+-]?\d+)(?![A-Za-zА-Яа-яЄєІіЇїҐґ\d])",
        )
    });
    let text = sub(&text, &E_NOTATION, |m| match scientific_words(cap(m, 2), cap(m, 3)) {
        Some(words) => format!("{}{words}", cap(m, 1)),
        None => whole(m).to_owned(),
    });

    let negative_power_range = compile(&format!(
        r"(^|[^\d])10−(\d+)\s*(?:-|–|—)\s*10−(\d+)(?:\s*(секунди|секунда|секунд|{unit_alt}))?(?![A-Za-zА-Яа-яЄєІіЇїҐґ])"
    ));
    let text = sub(&text, &negative_power_range, |m| {
        let (Some(low), Some(high)) = (
            exponent_words(&format!("-{}", cap(m, 2))),
            exponent_words(&format!("-{}", cap(m, 3))),
        ) else {
            return whole(m).to_owned();
        };
        let unit = if matched(m, 4) {
            match cap(m, 4) {
                "секунди" | "секунда" | "секунд" => " секунд".to_owned(),
                other => scientific_unit("10", other),
            }
        } else {
            String::new()
        };
        format!(
            "{}{}{unit}",
            cap(m, 1),
            range_connector(
                range_style,
                &format!("десяти у степені {low}"),
                &format!("десяти у степені {high}"),
                false
            )
        )
    });

    // In technical prose a tightly joined Unicode minus after 10 denotes a
    // negative power (10−9), not a numeric range from 10 to 9.
    let plain_negative_power = compile(&format!(r"(^|[^\d])10−(\d+)(?!\d){optional_unit}"));
    let text =
        sub(&text, &plain_negative_power, |m| match exponent_words(&format!("-{}", cap(m, 2))) {
            Some(exponent) => format!(
                "{}десять у степені {exponent}{}",
                cap(m, 1),
                scientific_unit("10", cap(m, 3))
            ),
            None => whole(m).to_owned(),
        });

    static POWER: Lazy<Regex> = Lazy::new(|| {
        compile(r"(^|[^A-Za-zА-Яа-яЄєІіЇїҐґ\d.,])([+-]?\d+(?:[.,]\d+)?)\s*\^\s*([+-]?\d+)(?!\d)")
    });
    sub(&text, &POWER, |m| {
        let base = signed_number_words(cap(m, 2), "nom", 'm');
        let exponent = exponent_words(cap(m, 3));
        match (base, exponent) {
            (Some(base), Some(exponent)) => {
                format!("{}{base} у степені {exponent}", cap(m, 1))
            }
            _ => whole(m).to_owned(),
        }
    })
}

/// Reads a `+` between digits as "plus".
pub(crate) fn normalize_math(text: &str) -> String {
    static RE: Lazy<Regex> = Lazy::new(|| compile(r"(\d)\s*\+\s*(?=\d)"));
    sub(text, &RE, |m| format!("{} плюс ", cap(m, 1)))
}

/// Reads decimal numbers.
pub(crate) fn normalize_decimals(text: &str) -> String {
    static RE: Lazy<Regex> = Lazy::new(|| compile(r"\b(\d+)[,.](\d+)\b"));
    sub(text, &RE, |m| decimal_to_words_or_digits(cap(m, 1), cap(m, 2)))
}

/// Reads a currency amount with more decimals than the currency has.
pub(crate) fn normalize_overprecise_currency_decimals(text: &str) -> String {
    static RE: Lazy<Regex> = Lazy::new(|| {
        compile_i(r"\b(\d+),(\d{3,})(?=\s*(?:грн|UAH|USD|EUR|GBP|[$€£₴]|долар|євро|фунт))")
    });
    sub(text, &RE, |m| {
        format!(
            "{} кома {}",
            number_digits_or_words(cap(m, 1)),
            number_to_words_digit_by_digit(cap(m, 2))
        )
    })
}

/// A scale word and the forms it takes.
struct Multiplier {
    forms: Forms,
    decimal: &'static str,
    feminine: bool,
}

#[rustfmt::skip]
static MULTIPLIERS: Lazy<HashMap<&'static str, Multiplier>> = Lazy::new(|| {
    [
        ("тис", Multiplier {
            forms: Forms { one: "тисяча", few: "тисячі", many: "тисяч" },
            decimal: "тисячі",
            feminine: true,
        }),
        ("млн", Multiplier {
            forms: Forms { one: "мільйон", few: "мільйони", many: "мільйонів" },
            decimal: "мільйона",
            feminine: false,
        }),
        ("млрд", Multiplier {
            forms: Forms { one: "мільярд", few: "мільярди", many: "мільярдів" },
            decimal: "мільярда",
            feminine: false,
        }),
        ("трлн", Multiplier {
            forms: Forms { one: "трильйон", few: "трильйони", many: "трильйонів" },
            decimal: "трильйона",
            feminine: false,
        }),
    ]
    .into_iter()
    .collect()
});

/// Reads `тис.`, `млн`, `млрд` and `трлн` after a number.
///
/// `governed_only` stops after the prepositional forms, which the pipeline runs
/// before currency handling.
pub(crate) fn normalize_multipliers(text: &str, governed_only: bool) -> String {
    static GOVERNED: Lazy<Regex> = Lazy::new(|| {
        compile_i(concat!(
            r"(^|[^А-Яа-яЄєІіЇїҐґ-])(Близько|близько|Після|після|Менше|менше|Більше|більше",
            r"|Серед|серед|Без|без|Від|від|До|до|Із|із)\s+(\d+(?:[.,]\d+)?)\s*",
            r"(тис|млн|млрд|трлн)\.?(?![а-яіїєґ])"
        ))
    });
    static RE: Lazy<Regex> =
        Lazy::new(|| compile_i(r"\b(\d+(?:[.,]\d+)?)\s*(тис|млн|млрд|трлн)(\.?)(?![а-яіїєґ])"));

    let text = sub(text, &GOVERNED, |m| {
        let key = lower_text(cap(m, 4));
        let Some(multiplier) = MULTIPLIERS.get(key.as_str()) else {
            return whole(m).to_owned();
        };
        let gender = if multiplier.feminine { 'f' } else { 'm' };
        let Some(words) = signed_number_words(cap(m, 3), "gen", gender) else {
            return whole(m).to_owned();
        };
        let unit =
            if cap(m, 3).contains(['.', ',']) { multiplier.decimal } else { multiplier.forms.many };
        format!("{}{} {words} {unit}", cap(m, 1), cap(m, 2))
    });
    if governed_only {
        return text;
    }
    sub_around(&text, &RE, |m, ctx| {
        let key = lower_text(cap(m, 2));
        let Some(multiplier) = MULTIPLIERS.get(key.as_str()) else {
            return whole(m).to_owned();
        };
        // A trailing period is kept only when it ends the text.
        let period = if matched(m, 3) && ctx.suffix.is_empty() { cap(m, 3) } else { "" };
        let num = cap(m, 1);
        if let Some(pos) = num.find(['.', ',']) {
            return match decimal_to_words(&num[..pos], &num[pos + 1..]) {
                Some(words) => format!("{words} {}{period}", multiplier.decimal),
                None => whole(m).to_owned(),
            };
        }
        let Some(n) = try_parse_u64(num) else {
            return format!(
                "{} {}{period}",
                number_to_words_digit_by_digit(num),
                multiplier.forms.many
            );
        };
        let mut words = split_words(&number_to_words(n));
        if multiplier.feminine {
            feminine_last(&mut words);
        }
        format!("{} {}{period}", join(&words), plural(n, &multiplier.forms))
    })
}

/// Reads dotted version numbers and short alphanumeric references.
pub(crate) fn normalize_versions(text: &str) -> String {
    static NAMED: Lazy<Regex> = Lazy::new(|| {
        compile(concat!(
            r"(^|[\s(\[{:,;])((?:(?:В|в)ерсі(?:я|ї|ю|єю)|(?:Р|р)еліз(?:у|ом)?|(?:В|в)ипуск(?:у|ом)?",
            r"|(?:П|п)ункт(?:у|ом|і|а)?|(?:Р|р)озділ(?:у|ом|і|а)?)\s+)(\d+(?:\.\d+)+)\b"
        ))
    });
    static AFTER_WORD: Lazy<Regex> =
        Lazy::new(|| compile(r"\b([A-Za-z][A-Za-z0-9_\-]*\s+)(\d+(?:\.\d+)+)\b"));
    static V_PREFIX: Lazy<Regex> = Lazy::new(|| compile(r"\b([vV])(\d+(?:\.\d+)+)\b"));
    static LETTER_DOT_NUMBER: Lazy<Regex> = Lazy::new(|| compile(r"\b([A-Za-z])\.(\d{1,6})\b"));
    static NUMBER_DOT_SUFFIX: Lazy<Regex> =
        Lazy::new(|| compile(r"\b(\d+)\.(\d+)([A-Za-z]{1,6})\b"));
    static DOTTED: Lazy<Regex> = Lazy::new(|| compile(r"\b\d+(?:\.\d+){2,}\b"));

    let text =
        sub(text, &NAMED, |m| format!("{}{}{}", cap(m, 1), cap(m, 2), read_dotted(cap(m, 3))));
    let text = sub(&text, &AFTER_WORD, |m| format!("{}{}", cap(m, 1), read_dotted(cap(m, 2))));
    let text = sub(&text, &V_PREFIX, |m| {
        format!("{} {}", spell_identifier_letters(cap(m, 1)), read_dotted(cap(m, 2)))
    });
    let text = sub(&text, &LETTER_DOT_NUMBER, |m| {
        format!(
            "{} крапка {}",
            spell_identifier_letters(cap(m, 1)),
            number_digits_or_words(cap(m, 2))
        )
    });
    let text = sub(&text, &NUMBER_DOT_SUFFIX, |m| {
        format!(
            "{} крапка {} {}",
            number_digits_or_words(cap(m, 1)),
            number_digits_or_words(cap(m, 2)),
            spell_identifier_letters(cap(m, 3))
        )
    });
    sub(&text, &DOTTED, |m| read_dotted(whole(m)))
}

/// Reads a leading minus sign as "minus".
pub(crate) fn normalize_negatives(text: &str) -> String {
    static RE: Lazy<Regex> = Lazy::new(|| compile(r"(^|[\s(\[])(?:-|−|–|—)(\d)"));
    sub(text, &RE, |m| format!("{}мінус {}", cap(m, 1), cap(m, 2)))
}

/// Reads every remaining bare number.
pub(crate) fn normalize_text_with_numbers(text: &str) -> String {
    static RE: Lazy<Regex> = Lazy::new(|| compile(r"\b\d+\b"));
    sub(text, &RE, |m| {
        let digits = whole(m);
        if digits.len() > 1 && digits.starts_with('0') {
            number_to_words_digit_by_digit(digits)
        } else {
            number_digits_or_words(digits)
        }
    })
}

/// Keeps helper imports referenced while passes are split across modules.
#[allow(dead_code)]
fn _unused() {
    let _ = (parse_i32, cap_start, plural_of, minutes_words);
}
