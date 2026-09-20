//! Ranges of every kind: temperatures, years, times, fractions, measurements,
//! currencies, percentages, paragraphs, school grades and bare numbers.

use fancy_regex::{Captures, Regex};
use std::fmt::Write as _;

use crate::uktextnorm::lexicon::{self, Forms, Gender};
use crate::uktextnorm::morphology::plural;
use crate::uktextnorm::numbers::{
    hours_words, minutes_words, number_to_words, number_to_words_case_str, number_words_for_case,
    ordinal_words, say_fraction, signed_number_words, take_spoken_sign, MINUTE_FORMS, SECOND_FORMS,
};
use crate::uktextnorm::patterns::{
    CURRENCY_TOKEN_ALT, MONTH_ALT, RANGE_PREFIX, RANGE_SEPARATOR, SIGNED_NUMBER, UNIT_ALT,
};
use crate::uktextnorm::re::{cap, compile, matched, sub, sub_around, sub_ctx, whole, MatchContext};
use crate::uktextnorm::readers::MEASUREMENTS;
use crate::uktextnorm::temperature::{
    temperature_quantity_words, temperature_range_unit, temperature_scale, Scale,
    TEMPERATURE_UNIT_PATTERN,
};
use crate::uktextnorm::text::{join, lower_text, parse_u64, try_parse_u64};
use crate::uktextnorm::RangeStyle;
use std::sync::LazyLock;

/// Nothing else may follow a number that closes a range.
const NUMBER_BOUNDARY: &str = r"(?![\d:/+\-−–—])(?![.,]\d)";

/// The lowercase word immediately before `prefix`, which often governs case.
pub(crate) fn preceding_word(prefix: &str) -> String {
    let text = lower_text(prefix);
    let text = text.trim_end();
    match text.rfind([' ', '\t', '\n', '\r', '(', '[', '{', ',', ';', ':']) {
        Some(boundary) => text[boundary + 1..].to_owned(),
        None => text.to_owned(),
    }
}

/// Joins the two ends of a range according to the style.
pub(crate) fn range_connector(
    style: RangeStyle,
    low: &str,
    high: &str,
    explicitly_from_to: bool,
) -> String {
    if style == RangeStyle::FromTo || explicitly_from_to {
        format!("від {low} до {high}")
    } else {
        format!("{low} {high}")
    }
}

/// A currency as a range reads it: its "many" form and its gender.
struct RangeCurrency {
    many: &'static str,
    gender: char,
}

fn range_currency(token: &str) -> Option<RangeCurrency> {
    let lowered = lower_text(token);
    if lowered == "грн" {
        return Some(RangeCurrency { many: "гривень", gender: 'f' });
    }
    lexicon::CURRENCIES
        .iter()
        .find(|entry| {
            lowered == lower_text(entry.code) || (!entry.symbol.is_empty() && token == entry.symbol)
        })
        .map(|entry| RangeCurrency {
            many: entry.main.many,
            gender: if entry.main_feminine { 'f' } else { 'm' },
        })
}

/// Reads a clock time, in the nominative or, for a `від ... до` range, genitive.
pub(crate) fn clock_time_words(
    hour_text: &str,
    minute_text: &str,
    second: Option<&str>,
    style: RangeStyle,
) -> String {
    let hour = parse_u64(hour_text);
    let minute = parse_u64(minute_text);
    let second = second.map(parse_u64).filter(|&s| s != 0);
    if style == RangeStyle::FromTo {
        let mut out = format!("{} години", ordinal_words(hour, "gen_f"));
        if minute != 0 {
            let _ = write!(out, " {} хвилин", join(&number_words_for_case(minute, "gen", 'f')));
        }
        if let Some(second) = second {
            let _ = write!(out, " {} секунд", join(&number_words_for_case(second, "gen", 'f')));
        }
        return out;
    }
    let mut out = hours_words(hour);
    if minute != 0 {
        let _ = write!(out, " {}", minutes_words(minute, &MINUTE_FORMS));
    }
    if let Some(second) = second {
        let _ = write!(out, " {}", minutes_words(second, &SECOND_FORMS));
    }
    out
}

/// Which preposition, if any, governs the range.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Governed {
    None,
    From,
    To,
    Near,
    On,
    In,
}

/// Shared range-reading state, so every closure sees the same rules.
struct Ranges {
    style: RangeStyle,
}

impl Ranges {
    /// The preposition governing this range, if the style makes it matter.
    fn governed(&self, prefix: &str, group1: &str, explicitly_from_to: bool) -> Governed {
        if self.style != RangeStyle::FromTo || explicitly_from_to {
            return Governed::None;
        }
        match preceding_word(&format!("{prefix}{group1}")).as_str() {
            "від" => Governed::From,
            "до" => Governed::To,
            "близько" => Governed::Near,
            "на" => Governed::On,
            "в" | "у" => Governed::In,
            _ => Governed::None,
        }
    }

    /// A governed range keeps its dash instead of becoming "від ... до".
    fn connector(
        &self,
        prefix: &str,
        group1: &str,
        low: &str,
        high: &str,
        explicitly_from_to: bool,
    ) -> String {
        if self.governed(prefix, group1, explicitly_from_to) == Governed::None {
            range_connector(self.style, low, high, explicitly_from_to)
        } else {
            format!("{low}–{high}")
        }
    }

    /// The case the bounds take.
    fn case(&self, prefix: &str, group1: &str, explicitly_from_to: bool) -> &'static str {
        let context = self.governed(prefix, group1, explicitly_from_to);
        // A "від ... до" range takes the genitive unless a preposition such as
        // "на" or "у" already governs the phrase.
        let genitive = (self.style == RangeStyle::FromTo
            && context != Governed::On
            && context != Governed::In)
            || explicitly_from_to;
        if genitive {
            "gen"
        } else {
            "nom"
        }
    }
}

/// One bound of a temperature range.
fn temperature_bound(token: &str, scale: &Scale, genitive: bool) -> Option<String> {
    if !genitive {
        return temperature_quantity_words(token, scale, "nom");
    }
    let words = signed_number_words(token, "gen", 'm')?;
    let name = if scale.genitive_name.is_empty() {
        String::new()
    } else {
        format!(" {}", scale.genitive_name)
    };
    Some(format!("{words} {}{name}", temperature_range_unit(token, scale)))
}

/// Reads every kind of numeric range.
pub(crate) fn normalize_ranges(text: &str, style: RangeStyle) -> String {
    let ranges = Ranges { style };
    let number = SIGNED_NUMBER;
    let separator = RANGE_SEPARATOR;
    let prefix = RANGE_PREFIX;
    let temperature_unit: &str = &TEMPERATURE_UNIT_PATTERN;
    let temperature_boundary = format!("{NUMBER_BOUNDARY}(?![A-Za-zА-Яа-яЄєІіЇїҐґ])");
    let unit_alt: &str = &UNIT_ALT;
    let currency_alt: &str = &CURRENCY_TOKEN_ALT;
    let not_letter = r"(?![A-Za-zА-Яа-яЄєІіЇїҐґ])";

    let say_temperature = |m: &Captures<'_, str>,
                           ctx: &MatchContext<'_>,
                           low_index: usize,
                           high_index: usize,
                           scale_index: usize,
                           explicitly_from_to: bool| {
        let case = ranges.case(ctx.prefix, cap(m, 1), explicitly_from_to);
        let (Some(low), Some(high)) = (
            signed_number_words(cap(m, low_index), case, 'm'),
            signed_number_words(cap(m, high_index), case, 'm'),
        ) else {
            return whole(m).to_owned();
        };
        let Some(scale) = temperature_scale(cap(m, scale_index)) else {
            return whole(m).to_owned();
        };
        let name = if scale.genitive_name.is_empty() {
            String::new()
        } else {
            format!(" {}", scale.genitive_name)
        };
        format!(
            "{}{} {}{name}",
            cap(m, 1),
            ranges.connector(ctx.prefix, cap(m, 1), &low, &high, explicitly_from_to),
            temperature_range_unit(cap(m, high_index), &scale)
        )
    };

    let explicit_repeated_temperature = compile(&format!(
        r"{prefix}від\s+({number})\s*({temperature_unit})\s+до\s+({number})\s*({temperature_unit}){temperature_boundary}"
    ));
    let text = sub_around(text, &explicit_repeated_temperature, |m, ctx| {
        let (Some(low_scale), Some(high_scale)) =
            (temperature_scale(cap(m, 3)), temperature_scale(cap(m, 5)))
        else {
            return whole(m).to_owned();
        };
        if low_scale.kind != high_scale.kind {
            let low = temperature_bound(cap(m, 2), &low_scale, true);
            let high = temperature_bound(cap(m, 4), &high_scale, true);
            return match (low, high) {
                (Some(low), Some(high)) => format!("{}від {low} до {high}", cap(m, 1)),
                _ => whole(m).to_owned(),
            };
        }
        say_temperature(m, ctx, 2, 4, 5, true)
    });

    let repeated_temperature = compile(&format!(
        r"{prefix}({number})\s*({temperature_unit})\s*{separator}\s*({number})\s*({temperature_unit}){temperature_boundary}"
    ));
    let text = sub_around(&text, &repeated_temperature, |m, ctx| {
        let (Some(low_scale), Some(high_scale)) =
            (temperature_scale(cap(m, 3)), temperature_scale(cap(m, 5)))
        else {
            return whole(m).to_owned();
        };
        if low_scale.kind != high_scale.kind {
            let genitive = style == RangeStyle::FromTo;
            let low = temperature_bound(cap(m, 2), &low_scale, genitive);
            let high = temperature_bound(cap(m, 4), &high_scale, genitive);
            return match (low, high) {
                (Some(low), Some(high)) => format!(
                    "{}{}",
                    cap(m, 1),
                    ranges.connector(ctx.prefix, cap(m, 1), &low, &high, false)
                ),
                _ => whole(m).to_owned(),
            };
        }
        say_temperature(m, ctx, 2, 4, 5, false)
    });

    let explicit_temperature = compile(&format!(
        r"{prefix}від\s+({number})\s+до\s+({number})\s*({temperature_unit}){temperature_boundary}"
    ));
    let text =
        sub_around(&text, &explicit_temperature, |m, ctx| say_temperature(m, ctx, 2, 3, 4, true));

    let temperature_range = compile(&format!(
        r"{prefix}({number})\s*{separator}\s*({number})\s*({temperature_unit}){temperature_boundary}"
    ));
    let text =
        sub_around(&text, &temperature_range, |m, ctx| say_temperature(m, ctx, 2, 3, 4, false));

    static PREPOSITIONAL_YEAR_RANGE: LazyLock<Regex> = LazyLock::new(|| {
        compile(concat!(
            r"(^|[^А-Яа-яЄєІіЇїҐґ])(У|у|В|в)\s+(\d{3,4})\s*(?:-|−|–|—)\s*(\d{3,4})",
            r"\s*(?:рр\.?|роки|роках|року|років)(?![\dа-яіїєґ])"
        ))
    });
    static BARE_PREPOSITIONAL_YEAR_RANGE: LazyLock<Regex> = LazyLock::new(|| {
        compile(r"(^|[^А-Яа-яЄєІіЇїҐґ])(У|у|В|в)\s+(\d{4})\s*(?:-|−|–|—)\s*(\d{4})(?![\dа-яіїєґ])")
    });
    let say_prepositional_year_range = |m: &Captures<'_, str>| {
        format!(
            "{}{} період від {} до {} року",
            cap(m, 1),
            cap(m, 2),
            ordinal_words(parse_u64(cap(m, 3)), "gen"),
            ordinal_words(parse_u64(cap(m, 4)), "gen")
        )
    };
    let text = sub(&text, &PREPOSITIONAL_YEAR_RANGE, say_prepositional_year_range);
    // Without an explicit year word, only a four-digit span counts as years.
    let text = sub(&text, &BARE_PREPOSITIONAL_YEAR_RANGE, say_prepositional_year_range);

    /// `1990–95` means 1990–1995, rolling into the next century if needed.
    fn expanded_short_year(first: u64, short_second: u64) -> u64 {
        let mut second = (first / 100) * 100 + short_second;
        if second < first {
            second += 100;
        }
        second
    }

    static ABBREVIATED_DECADE_RANGE: LazyLock<Regex> = LazyLock::new(|| {
        compile(concat!(
            r"\b((?:19|20)\d{2})\s*(?:-|–|—)\s*(\d{2})(?:-|–|—)?(х|их|і|ї)",
            r"(?:\s+(роках|років|роки))?(?![\dА-Яа-яЄєІіЇїҐґ])"
        ))
    });
    let text = sub(&text, &ABBREVIATED_DECADE_RANGE, |m| {
        let first = parse_u64(cap(m, 1));
        let second = expanded_short_year(first, parse_u64(cap(m, 2)));
        if first / 100 != second / 100 || first % 10 != 0 || second % 10 != 0 {
            return whole(m).to_owned();
        }
        let form = if matches!(cap(m, 3), "і" | "ї") { "nom_pl" } else { "pl" };
        let year_word = if matched(m, 4) {
            cap(m, 4)
        } else if form == "nom_pl" {
            "роки"
        } else {
            "роках"
        };
        format!(
            "{}–{} {year_word} {} століття",
            ordinal_words(first % 100, form),
            ordinal_words(second % 100, form),
            ordinal_words(first / 100 + 1, "gen")
        )
    });

    static ABBREVIATED_PREPOSITIONAL_YEAR_RANGE: LazyLock<Regex> = LazyLock::new(|| {
        compile(concat!(
            r"(^|[^А-Яа-яЄєІіЇїҐґ])(У|у|В|в)\s+((?:19|20)\d{2})\s*(?:-|–|—)\s*(\d{2})",
            r"\s*(?:рр?\.?|роки|роках|року|років)(?![\dа-яіїєґ])"
        ))
    });
    let text = sub(&text, &ABBREVIATED_PREPOSITIONAL_YEAR_RANGE, |m| {
        let first = parse_u64(cap(m, 3));
        let second = expanded_short_year(first, parse_u64(cap(m, 4)));
        format!(
            "{}{} період від {} до {} року",
            cap(m, 1),
            cap(m, 2),
            ordinal_words(first, "gen"),
            ordinal_words(second, "gen")
        )
    });

    static ABBREVIATED_YEAR_RANGE: LazyLock<Regex> = LazyLock::new(|| {
        compile(
            r"\b((?:19|20)\d{2})\s*(?:-|–|—)\s*(\d{2})\s*(?:рр?\.?|роки|роках|року|років)(?![\dа-яіїєґ])",
        )
    });
    let text = sub(&text, &ABBREVIATED_YEAR_RANGE, |m| {
        let first = parse_u64(cap(m, 1));
        let second = expanded_short_year(first, parse_u64(cap(m, 2)));
        if style == RangeStyle::FromTo {
            format!("від {} до {} року", ordinal_words(first, "gen"), ordinal_words(second, "gen"))
        } else {
            format!("{}–{} роки", ordinal_words(first, "nom_m"), ordinal_words(second, "nom_m"))
        }
    });

    static YEAR_RANGE: LazyLock<Regex> = LazyLock::new(|| {
        compile(r"\b(\d{3,4})\s*(?:-|−|–|—)\s*(\d{3,4})\s*(?:рр\.?|роки)(?![а-яіїєґ])")
    });
    let text = sub_ctx(&text, &YEAR_RANGE, |m, prefix| {
        let low = parse_u64(cap(m, 1));
        let high = parse_u64(cap(m, 2));
        if style != RangeStyle::FromTo {
            return format!(
                "{} {} роки",
                ordinal_words(low, "nom_m"),
                ordinal_words(high, "nom_m")
            );
        }
        match preceding_word(prefix).as_str() {
            "на" => format!(
                "період від {} до {} року",
                ordinal_words(low, "gen"),
                ordinal_words(high, "gen")
            ),
            "близько" | "до" | "від" => {
                format!("{}–{} року", ordinal_words(low, "gen"), ordinal_words(high, "gen"))
            }
            _ => {
                format!("від {} до {} року", ordinal_words(low, "gen"), ordinal_words(high, "gen"))
            }
        }
    });

    let time_range = compile(&format!(
        r"{prefix}(\d{{1,2}}):([0-5]\d)(?::([0-5]\d))?\s*{separator}\s*(\d{{1,2}}):([0-5]\d)(?::([0-5]\d))?(?![\d:])"
    ));
    let text = sub(&text, &time_range, |m| {
        if parse_u64(cap(m, 2)) > 23 || parse_u64(cap(m, 5)) > 23 {
            return whole(m).to_owned();
        }
        let low = clock_time_words(cap(m, 2), cap(m, 3), matched(m, 4).then(|| cap(m, 4)), style);
        let high = clock_time_words(cap(m, 5), cap(m, 6), matched(m, 7).then(|| cap(m, 7)), style);
        format!("{}{}", cap(m, 1), range_connector(style, &low, &high, false))
    });

    let fraction_range =
        compile(&format!(r"{prefix}(\d+)/(\d+)\s*{separator}\s*(\d+)/(\d+)(?![\d/])"));
    let text = sub(&text, &fraction_range, |m| {
        let values = [2, 3, 4, 5].map(|i| try_parse_u64(cap(m, i)));
        let [Some(n1), Some(d1), Some(n2), Some(d2)] = values else {
            return whole(m).to_owned();
        };
        if d1 == 0 || d2 == 0 {
            return whole(m).to_owned();
        }
        let say = |numerator: u64, denominator: u64| {
            if style != RangeStyle::FromTo {
                return say_fraction(numerator, denominator);
            }
            let form = if numerator % 10 == 1 && numerator % 100 != 11 { "gen_f" } else { "pl" };
            format!(
                "{} {}",
                join(&number_words_for_case(numerator, "gen", 'f')),
                ordinal_words(denominator, form)
            )
        };
        format!("{}{}", cap(m, 1), range_connector(style, &say(n1, d1), &say(n2, d2), false))
    });

    let say_measurement = |m: &Captures<'_, str>,
                           ctx: &MatchContext<'_>,
                           low_index: usize,
                           high_index: usize,
                           unit_index: usize,
                           explicitly_from_to: bool| {
        let Some(measurement) = MEASUREMENTS.get(cap(m, unit_index)) else {
            return whole(m).to_owned();
        };
        let gender = if measurement.gender == Gender::Feminine { 'f' } else { 'm' };
        let case = ranges.case(ctx.prefix, cap(m, 1), explicitly_from_to);
        let (Some(low), Some(high)) = (
            signed_number_words(cap(m, low_index), case, gender),
            signed_number_words(cap(m, high_index), case, gender),
        ) else {
            return whole(m).to_owned();
        };
        let mut unit = measurement.forms.many;
        let context = ranges.governed(ctx.prefix, cap(m, 1), explicitly_from_to);
        let agrees_with_upper =
            (style == RangeStyle::Compact || context == Governed::On || context == Governed::In)
                && !explicitly_from_to;
        if agrees_with_upper {
            let mut upper = cap(m, high_index);
            take_spoken_sign(&mut upper);
            if upper.contains(['.', ',']) {
                unit = measurement.forms.few;
            } else if let Some(value) = try_parse_u64(upper) {
                unit = plural(value, &measurement.forms);
            }
        }
        format!(
            "{}{} {unit}",
            cap(m, 1),
            ranges.connector(ctx.prefix, cap(m, 1), &low, &high, explicitly_from_to)
        )
    };

    let repeated_unit_range = compile(&format!(
        r"{prefix}({number})\s*({unit_alt})\s*{separator}\s*({number})\s*({unit_alt}){not_letter}"
    ));
    let text = sub_around(&text, &repeated_unit_range, |m, ctx| {
        if cap(m, 3) == cap(m, 5) {
            say_measurement(m, ctx, 2, 4, 5, false)
        } else {
            whole(m).to_owned()
        }
    });
    let explicit_repeated_unit_range = compile(&format!(
        r"{prefix}від\s+({number})\s*({unit_alt})\s+до\s+({number})\s*({unit_alt}){not_letter}"
    ));
    let text = sub_around(&text, &explicit_repeated_unit_range, |m, ctx| {
        if cap(m, 3) == cap(m, 5) {
            say_measurement(m, ctx, 2, 4, 5, true)
        } else {
            whole(m).to_owned()
        }
    });
    let explicit_unit_range =
        compile(&format!(r"{prefix}від\s+({number})\s+до\s+({number})\s*({unit_alt}){not_letter}"));
    let text =
        sub_around(&text, &explicit_unit_range, |m, ctx| say_measurement(m, ctx, 2, 3, 4, true));
    let explicit_separator_unit_range = compile(&format!(
        r"{prefix}від\s+({number})\s*{separator}\s*({number})\s*({unit_alt}){not_letter}"
    ));
    let text = sub_around(&text, &explicit_separator_unit_range, |m, ctx| {
        say_measurement(m, ctx, 2, 3, 4, true)
    });
    let unit_range = compile(&format!(
        r"{prefix}({number})\s*{separator}\s*({number})\s*({unit_alt}){not_letter}"
    ));
    let text = sub_around(&text, &unit_range, |m, ctx| say_measurement(m, ctx, 2, 3, 4, false));

    let say_currency = |m: &Captures<'_, str>,
                        ctx: &MatchContext<'_>,
                        low_index: usize,
                        high_index: usize,
                        currency_index: usize,
                        explicitly_from_to: bool| {
        let Some(currency) = range_currency(cap(m, currency_index)) else {
            return whole(m).to_owned();
        };
        let case = ranges.case(ctx.prefix, cap(m, 1), explicitly_from_to);
        let (Some(low), Some(high)) = (
            signed_number_words(cap(m, low_index), case, currency.gender),
            signed_number_words(cap(m, high_index), case, currency.gender),
        ) else {
            return whole(m).to_owned();
        };
        format!(
            "{}{} {}",
            cap(m, 1),
            ranges.connector(ctx.prefix, cap(m, 1), &low, &high, explicitly_from_to),
            currency.many
        )
    };
    let same_currency = |a: &str, b: &str| match (range_currency(a), range_currency(b)) {
        (Some(first), Some(second)) => first.many == second.many,
        _ => false,
    };

    let repeated_prefix_currency = compile(&format!(
        r"{prefix}({currency_alt})\s*({number})\s*{separator}\s*({currency_alt})\s*({number}){NUMBER_BOUNDARY}"
    ));
    let text = sub_around(&text, &repeated_prefix_currency, |m, ctx| {
        if same_currency(cap(m, 2), cap(m, 4)) {
            say_currency(m, ctx, 3, 5, 4, false)
        } else {
            whole(m).to_owned()
        }
    });
    let prefix_currency_range = compile(&format!(
        r"{prefix}({currency_alt})\s*({number})\s*{separator}\s*({number}){NUMBER_BOUNDARY}"
    ));
    let text =
        sub_around(&text, &prefix_currency_range, |m, ctx| say_currency(m, ctx, 3, 4, 2, false));
    let repeated_suffix_currency = compile(&format!(
        r"{prefix}({number})\s*({currency_alt})\s*{separator}\s*({number})\s*({currency_alt}){not_letter}"
    ));
    let text = sub_around(&text, &repeated_suffix_currency, |m, ctx| {
        if same_currency(cap(m, 3), cap(m, 5)) {
            say_currency(m, ctx, 2, 4, 5, false)
        } else {
            whole(m).to_owned()
        }
    });
    let explicit_repeated_suffix_currency = compile(&format!(
        r"{prefix}від\s+({number})\s*({currency_alt})\s+до\s+({number})\s*({currency_alt}){not_letter}"
    ));
    let text = sub_around(&text, &explicit_repeated_suffix_currency, |m, ctx| {
        if same_currency(cap(m, 3), cap(m, 5)) {
            say_currency(m, ctx, 2, 4, 5, true)
        } else {
            whole(m).to_owned()
        }
    });
    let explicit_currency_range = compile(&format!(
        r"{prefix}від\s+({number})\s+до\s+({number})\s*({currency_alt}){not_letter}"
    ));
    let text =
        sub_around(&text, &explicit_currency_range, |m, ctx| say_currency(m, ctx, 2, 3, 4, true));
    let suffix_currency_range = compile(&format!(
        r"{prefix}({number})\s*{separator}\s*({number})\s*({currency_alt}){not_letter}"
    ));
    let text =
        sub_around(&text, &suffix_currency_range, |m, ctx| say_currency(m, ctx, 2, 3, 4, false));

    let say_percent = |m: &Captures<'_, str>,
                       ctx: &MatchContext<'_>,
                       low_index: usize,
                       high_index: usize,
                       explicitly_from_to: bool| {
        let case = ranges.case(ctx.prefix, cap(m, 1), explicitly_from_to);
        let (Some(low), Some(high)) = (
            signed_number_words(cap(m, low_index), case, 'm'),
            signed_number_words(cap(m, high_index), case, 'm'),
        ) else {
            return whole(m).to_owned();
        };
        let mut unit = "відсотків";
        let context = ranges.governed(ctx.prefix, cap(m, 1), explicitly_from_to);
        if context == Governed::On || context == Governed::In {
            let mut high_token = cap(m, high_index);
            take_spoken_sign(&mut high_token);
            if let Some(value) = try_parse_u64(high_token) {
                unit = plural(
                    value,
                    &Forms {
                        one: "відсоток", few: "відсотки", many: "відсотків"
                    },
                );
            }
        }
        format!(
            "{}{} {unit}",
            cap(m, 1),
            ranges.connector(ctx.prefix, cap(m, 1), &low, &high, explicitly_from_to)
        )
    };

    let repeated_percent_range =
        compile(&format!(r"{prefix}({number})\s*%\s*{separator}\s*({number})\s*%(?!\w)"));
    let text =
        sub_around(&text, &repeated_percent_range, |m, ctx| say_percent(m, ctx, 2, 3, false));
    let explicit_repeated_percent_range =
        compile(&format!(r"{prefix}від\s+({number})\s*%\s+до\s*({number})\s*%(?!\w)"));
    let text = sub_around(&text, &explicit_repeated_percent_range, |m, ctx| {
        say_percent(m, ctx, 2, 3, true)
    });
    let explicit_percent_range =
        compile(&format!(r"{prefix}від\s+({number})\s+до\s+({number})\s*%(?!\w)"));
    let text = sub_around(&text, &explicit_percent_range, |m, ctx| say_percent(m, ctx, 2, 3, true));
    let explicit_separator_percent_range =
        compile(&format!(r"{prefix}від\s+({number})\s*{separator}\s*({number})\s*%(?!\w)"));
    let text = sub_around(&text, &explicit_separator_percent_range, |m, ctx| {
        say_percent(m, ctx, 2, 3, true)
    });
    let percent_range =
        compile(&format!(r"{prefix}({number})\s*{separator}\s*({number})\s*%(?!\w)"));
    let text = sub_around(&text, &percent_range, |m, ctx| say_percent(m, ctx, 2, 3, false));

    let paragraph_range =
        compile(&format!(r"{prefix}(?:§§|§)\s*(\d+)\s*{separator}\s*(\d+)(?!\d)"));
    let text = sub(&text, &paragraph_range, |m| {
        let low = parse_u64(cap(m, 2));
        let high = parse_u64(cap(m, 3));
        if style == RangeStyle::FromTo {
            format!(
                "{}від {} до {} параграфа",
                cap(m, 1),
                ordinal_words(low, "gen"),
                ordinal_words(high, "gen")
            )
        } else {
            format!("{}параграфи {} {}", cap(m, 1), number_to_words(low), number_to_words(high))
        }
    });

    let school_grade_range = compile(&format!(
        r"{prefix}(\d{{1,2}})\s*{separator}\s*(\d{{1,2}})\s+(класах|класів)(?![А-Яа-яЄєІіЇїҐґ])"
    ));
    let text = sub_ctx(&text, &school_grade_range, |m, prefix| {
        let word = preceding_word(&format!("{prefix}{}", cap(m, 1)));
        let noun = cap(m, 4);
        let governed = (noun == "класах" && (word == "у" || word == "в"))
            || (noun == "класів" && word == "учнів");
        if !governed {
            return whole(m).to_owned();
        }
        let first = parse_u64(cap(m, 2));
        let second = parse_u64(cap(m, 3));
        if first == 0 || second == 0 {
            return whole(m).to_owned();
        }
        format!(
            "{}{}–{} {noun}",
            cap(m, 1),
            ordinal_words(first, "pl"),
            ordinal_words(second, "pl")
        )
    });

    let say_bare = |m: &Captures<'_, str>, ctx: &MatchContext<'_>, explicitly_from_to: bool| {
        // A four-digit bound followed by a short one is a year span, read elsewhere.
        if !explicitly_from_to && cap(m, 2).len() == 4 && cap(m, 3).len() <= 2 {
            if let Some(year) = try_parse_u64(cap(m, 2)) {
                if (1000..=2999).contains(&year) {
                    return whole(m).to_owned();
                }
            }
        }
        let context = ranges.governed(ctx.prefix, cap(m, 1), explicitly_from_to);
        if context == Governed::In && ctx.suffix.starts_with(" класах") {
            if let (Some(low), Some(high)) = (try_parse_u64(cap(m, 2)), try_parse_u64(cap(m, 3))) {
                return format!(
                    "{}{}–{}",
                    cap(m, 1),
                    number_to_words_case_str(low, "prep"),
                    number_to_words_case_str(high, "prep")
                );
            }
        }
        let case = ranges.case(ctx.prefix, cap(m, 1), explicitly_from_to);
        let gender = if context == Governed::In && ctx.suffix.starts_with(" лінії") {
            'f'
        } else {
            'm'
        };
        let (Some(low), Some(high)) = (
            signed_number_words(cap(m, 2), case, gender),
            signed_number_words(cap(m, 3), case, gender),
        ) else {
            return whole(m).to_owned();
        };
        format!(
            "{}{}",
            cap(m, 1),
            ranges.connector(ctx.prefix, cap(m, 1), &low, &high, explicitly_from_to)
        )
    };

    let not_a_date = format!(r"(?!\s+(?:{MONTH_ALT})(?:\s|$))");
    let explicit_bare_range = compile(&format!(
        r"{prefix}від\s+({number})\s+до\s+({number}){NUMBER_BOUNDARY}{not_a_date}"
    ));
    let text = sub_around(&text, &explicit_bare_range, |m, ctx| say_bare(m, ctx, true));
    let explicit_separator_bare_range =
        compile(&format!(r"{prefix}від\s+({number})\s*{separator}\s*({number}){NUMBER_BOUNDARY}"));
    let text = sub_around(&text, &explicit_separator_bare_range, |m, ctx| say_bare(m, ctx, true));
    let approximate_bare_range = compile(&format!(
        r"{prefix}(Понад|понад)\s+({number})\s*{separator}\s*({number}){NUMBER_BOUNDARY}"
    ));
    let text = sub(&text, &approximate_bare_range, |m| {
        let (Some(low), Some(high)) = (
            signed_number_words(cap(m, 3), "nom", 'm'),
            signed_number_words(cap(m, 4), "nom", 'm'),
        ) else {
            return whole(m).to_owned();
        };
        format!("{}{} {low} чи {high}", cap(m, 1), cap(m, 2))
    });
    let bare_range = compile(&format!(
        r"{prefix}({number})\s*{separator}\s*({number}){NUMBER_BOUNDARY}{not_a_date}"
    ));
    sub_around(&text, &bare_range, |m, ctx| say_bare(m, ctx, false))
}
