//! Dates, date ranges, durations and year references.

use fancy_regex::{Captures, Regex};
use once_cell::sync::Lazy;
use std::collections::HashMap;

use crate::uktextnorm::lexicon::Forms;
use crate::uktextnorm::morphology::plural;
use crate::uktextnorm::numbers::{
    decimal_to_words, hours_words, minutes_words, number_to_words, number_words_for_gender,
    ordinal_words, MINUTE_FORMS, SECOND_FORMS,
};
use crate::uktextnorm::patterns::{DATE_DAY_RANGE_RE, DATE_SPELLED_RE, MONTH_ALT};
use crate::uktextnorm::re::{cap, compile, compile_i, matched, sub, sub_ctx, whole};
use crate::uktextnorm::text::{lower_text, parse_i32, parse_u64};
use crate::uktextnorm::validation::{is_valid_date, is_valid_iso_week};
use crate::uktextnorm::{DateStyle, NumericDateOrder, RangeStyle};

use super::ranges::{preceding_word, range_connector};

/// Month names in the genitive, as a date uses them.
#[rustfmt::skip]
const MONTHS_GENITIVE: [&str; 12] = [
    "січня", "лютого", "березня", "квітня", "травня", "червня", "липня", "серпня", "вересня",
    "жовтня", "листопада", "грудня",
];

/// Month names in the nominative.
#[rustfmt::skip]
const MONTHS_NOMINATIVE: [&str; 12] = [
    "січень", "лютий", "березень", "квітень", "травень", "червень", "липень", "серпень",
    "вересень", "жовтень", "листопад", "грудень",
];

/// Every month spelling a date can use, abbreviated or inflected.
const MONTH_ANY: &str = concat!(
    r"(?:січ(?:ень|ня|ні)?|лют(?:ий|ого|ому)?|бер(?:езень|езня|езні)?|квіт(?:ень|ня|ні)?",
    r"|трав(?:ень|ня|ні)?|черв(?:ень|ня|ні)?|лип(?:ень|ня|ні)?|серп(?:ень|ня|ні)?",
    r"|вер(?:есень|есня|есні)?|жовт(?:ень|ня|ні)?|лист(?:опад|опада|опаді)?|груд(?:ень|ня|ні)?)"
);

/// Maps an abbreviated or inflected month to its genitive form.
#[rustfmt::skip]
static MONTH_GENITIVE_BY_TOKEN: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    [
        ("січ", "січня"), ("січня", "січня"), ("лют", "лютого"), ("лютого", "лютого"),
        ("бер", "березня"), ("березня", "березня"), ("квіт", "квітня"), ("квітня", "квітня"),
        ("трав", "травня"), ("травня", "травня"), ("черв", "червня"), ("червня", "червня"),
        ("лип", "липня"), ("липня", "липня"), ("серп", "серпня"), ("серпня", "серпня"),
        ("вер", "вересня"), ("вересня", "вересня"), ("жовт", "жовтня"), ("жовтня", "жовтня"),
        ("лист", "листопада"), ("листопада", "листопада"), ("груд", "грудня"),
        ("грудня", "грудня"),
    ]
    .into_iter()
    .collect()
});

/// Maps an abbreviated or inflected month to its nominative form.
#[rustfmt::skip]
static MONTH_NOMINATIVE_BY_TOKEN: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    [
        ("січ", "січень"), ("січень", "січень"), ("січня", "січень"), ("лют", "лютий"),
        ("лютий", "лютий"), ("лютого", "лютий"), ("бер", "березень"), ("березень", "березень"),
        ("березня", "березень"), ("квіт", "квітень"), ("квітень", "квітень"),
        ("квітня", "квітень"), ("трав", "травень"), ("травень", "травень"),
        ("травня", "травень"), ("черв", "червень"), ("червень", "червень"),
        ("червня", "червень"), ("лип", "липень"), ("липень", "липень"), ("липня", "липень"),
        ("серп", "серпень"), ("серпень", "серпень"), ("серпня", "серпень"),
        ("вер", "вересень"), ("вересень", "вересень"), ("вересня", "вересень"),
        ("жовт", "жовтень"), ("жовтень", "жовтень"), ("жовтня", "жовтень"),
        ("лист", "листопад"), ("листопад", "листопад"), ("листопада", "листопад"),
        ("груд", "грудень"), ("грудень", "грудень"), ("грудня", "грудень"),
    ]
    .into_iter()
    .collect()
});

fn normalize_month_token(token: &str) -> String {
    lower_text(&token.replace('.', ""))
}

fn month_name(token: &str) -> String {
    let key = normalize_month_token(token);
    MONTH_GENITIVE_BY_TOKEN.get(key.as_str()).map_or(key.clone(), |&m| m.to_owned())
}

fn month_nominative(token: &str) -> String {
    let key = normalize_month_token(token);
    MONTH_NOMINATIVE_BY_TOKEN.get(key.as_str()).map_or(key.clone(), |&m| m.to_owned())
}

/// Two-digit years below 50 belong to this century.
fn expand_short_year(year: &str) -> u64 {
    let value = parse_u64(year);
    if year.len() == 2 {
        if value < 50 {
            2000 + value
        } else {
            1900 + value
        }
    } else {
        value
    }
}

/// The state the date passes share, so the closures stay readable.
struct Dates {
    style: DateStyle,
    validate: bool,
    range_style: RangeStyle,
    order: NumericDateOrder,
}

impl Dates {
    fn day_words(&self, day: &str, formal_form: &str) -> String {
        let form = if self.style == DateStyle::Spoken { "gen" } else { formal_form };
        ordinal_words(parse_u64(day), form)
    }

    fn range_day_words(&self, day: &str) -> String {
        if self.range_style == RangeStyle::FromTo {
            ordinal_words(parse_u64(day), "gen")
        } else {
            self.day_words(day, "nom_n")
        }
    }

    /// Joins the two ends of a date range, honouring a governing preposition.
    fn range_connector(&self, prefix: &str, low: &str, high: &str) -> String {
        if self.range_style == RangeStyle::FromTo {
            match preceding_word(prefix).as_str() {
                "на" | "в" | "у" => return format!("період від {low} до {high}"),
                "до" | "від" | "близько" => return format!("{low}–{high}"),
                _ => {}
            }
        }
        range_connector(self.range_style, low, high, false)
    }

    /// Reads a full date, or `None` when it is not a real one.
    fn full_date_words(
        &self,
        day: &str,
        month: &str,
        year: &str,
        forced_day_form: &str,
    ) -> Option<String> {
        let day_value = parse_i32(day);
        let month_value = parse_i32(month);
        let year_value = expand_short_year(year);
        if !(1..=12).contains(&month_value) {
            return None;
        }
        if self.validate && !is_valid_date(day_value, month_value, year_value as i32) {
            return None;
        }
        let spoken_day = if forced_day_form.is_empty() {
            self.day_words(day, "nom_n")
        } else {
            ordinal_words(parse_u64(day), forced_day_form)
        };
        Some(format!(
            "{spoken_day} {} {} року",
            MONTHS_GENITIVE[(month_value - 1) as usize],
            ordinal_words(year_value, "gen")
        ))
    }

    fn ordered_date_words(
        &self,
        first: &str,
        second: &str,
        year: &str,
        forced_day_form: &str,
    ) -> Option<String> {
        if self.order == NumericDateOrder::MonthDayYear {
            self.full_date_words(second, first, year, forced_day_form)
        } else {
            self.full_date_words(first, second, year, forced_day_form)
        }
    }

    /// A slash date such as `04/29/02` cannot be day-first, so read the only
    /// valid order without disturbing ambiguous dates such as `04/05/02`.
    fn slash_date_words(
        &self,
        first: &str,
        second: &str,
        year: &str,
        forced_day_form: &str,
    ) -> Option<String> {
        if self.order != NumericDateOrder::MonthDayYear
            && parse_i32(first) <= 12
            && parse_i32(second) > 12
        {
            return self.full_date_words(second, first, year, forced_day_form);
        }
        self.ordered_date_words(first, second, year, forced_day_form)
    }
}

fn time_words(hour: u64, minute: u64, second: Option<u64>) -> String {
    let mut out = hours_words(hour);
    if minute != 0 {
        out.push_str(&format!(" {}", minutes_words(minute, &MINUTE_FORMS)));
    }
    if let Some(second) = second.filter(|&s| s != 0) {
        out.push_str(&format!(" {}", minutes_words(second, &SECOND_FORMS)));
    }
    out
}

/// Reads dates, date ranges, ISO durations and year references.
pub(crate) fn normalize_dates(
    text: &str,
    style: DateStyle,
    validate: bool,
    range_style: RangeStyle,
    order: NumericDateOrder,
) -> String {
    let dates = Dates { style, validate, range_style, order };

    static ISO_DURATION: Lazy<Regex> = Lazy::new(|| {
        compile_i(concat!(
            r"\bP(?:(\d+(?:[.,]\d+)?)Y)?(?:(\d+(?:[.,]\d+)?)M)?(?:(\d+(?:[.,]\d+)?)W)?",
            r"(?:(\d+(?:[.,]\d+)?)D)?(?:T(?:(\d+(?:[.,]\d+)?)H)?(?:(\d+(?:[.,]\d+)?)M)?",
            r"(?:(\d+(?:[.,]\d+)?)S)?)?\b"
        ))
    });
    let text = sub(text, &ISO_DURATION, |m| {
        const FORMS: [Forms; 7] = [
            Forms { one: "рік", few: "роки", many: "років" },
            Forms { one: "місяць", few: "місяці", many: "місяців" },
            Forms { one: "тиждень", few: "тижні", many: "тижнів" },
            Forms { one: "день", few: "дні", many: "днів" },
            Forms { one: "година", few: "години", many: "годин" },
            Forms { one: "хвилина", few: "хвилини", many: "хвилин" },
            Forms { one: "секунда", few: "секунди", many: "секунд" },
        ];
        const DECIMAL_FORMS: [&str; 7] =
            ["року", "місяця", "тижня", "дня", "години", "хвилини", "секунди"];
        const GENDERS: [char; 7] = ['m', 'm', 'm', 'm', 'f', 'f', 'f'];
        let mut parts = Vec::new();
        for i in 1..=FORMS.len() {
            if !matched(m, i) {
                continue;
            }
            let token = cap(m, i);
            if let Some(decimal) = token.find(['.', ',']) {
                let words =
                    decimal_to_words(&token[..decimal], &token[decimal + 1..]).unwrap_or_default();
                parts.push(format!("{words} {}", DECIMAL_FORMS[i - 1]));
            } else {
                let value = parse_u64(token);
                parts.push(format!(
                    "{} {}",
                    number_words_for_gender(value, GENDERS[i - 1]),
                    plural(value, &FORMS[i - 1])
                ));
            }
        }
        if parts.is_empty() {
            whole(m).to_owned()
        } else {
            parts.join(" ")
        }
    });

    static ISO_WEEK: Lazy<Regex> = Lazy::new(|| compile_i(r"\b(\d{4})-W(\d{2})(?:-(\d))?\b"));
    let text = sub(&text, &ISO_WEEK, |m| {
        let year = parse_u64(cap(m, 1));
        let week = parse_u64(cap(m, 2));
        let day = if matched(m, 3) { parse_u64(cap(m, 3)) } else { 0 };
        if !is_valid_iso_week(year as i32, week as i32) || day > 7 {
            return whole(m).to_owned();
        }
        let prefix = if day != 0 {
            format!("{} день ", ordinal_words(day, "nom_m"))
        } else {
            String::new()
        };
        format!("{prefix}{} тижня {} року", ordinal_words(week, "gen"), ordinal_words(year, "gen"))
    });

    static ISO_ORDINAL_DATE: Lazy<Regex> = Lazy::new(|| compile(r"\b(\d{4})-(\d{3})\b"));
    let text = sub(&text, &ISO_ORDINAL_DATE, |m| {
        let year = parse_u64(cap(m, 1));
        let day = parse_u64(cap(m, 2));
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        if day < 1 || day > if leap { 366 } else { 365 } {
            return whole(m).to_owned();
        }
        format!("{} день {} року", ordinal_words(day, "nom_m"), ordinal_words(year, "gen"))
    });

    static ISO_DATETIME: Lazy<Regex> = Lazy::new(|| {
        compile_i(concat!(
            r"\b(\d{4})-(\d{2})-(\d{2})[Tt ](\d{2}):([0-5]\d)(?::([0-5]\d))?",
            r"(?:(Z)|(?:(UTC|GMT)\s*)?([+-])(\d{2})(?::?(\d{2}))?)?(?![A-Za-z0-9:+-])"
        ))
    });
    let text = sub(&text, &ISO_DATETIME, |m| {
        let Some(date) = dates.full_date_words(cap(m, 3), cap(m, 2), cap(m, 1), "") else {
            return whole(m).to_owned();
        };
        let hour = parse_u64(cap(m, 4));
        if hour > 23 {
            return whole(m).to_owned();
        }
        let second = matched(m, 6).then(|| parse_u64(cap(m, 6)));
        let mut out = format!("{date} о {}", time_words(hour, parse_u64(cap(m, 5)), second));
        if matched(m, 7) {
            out.push_str(" за всесвітнім координованим часом");
        } else if matched(m, 9) {
            let offset_hour = parse_u64(cap(m, 10));
            let offset_minute = if matched(m, 11) { parse_u64(cap(m, 11)) } else { 0 };
            if offset_hour > 14 || offset_minute > 59 || (offset_hour == 14 && offset_minute != 0) {
                return whole(m).to_owned();
            }
            let sign = if cap(m, 9) == "+" { "плюс " } else { "мінус " };
            out.push_str(&format!(
                " за часовим поясом {sign}{} {}",
                number_to_words(offset_hour),
                plural(offset_hour, &Forms { one: "година", few: "години", many: "годин" })
            ));
            if offset_minute != 0 {
                out.push_str(&format!(" {}", minutes_words(offset_minute, &MINUTE_FORMS)));
            }
        }
        out
    });

    static CROSS_MONTH_RANGE: Lazy<Regex> = Lazy::new(|| {
        compile(&format!(
            r"\b(\d{{1,2}})\s+({MONTH_ALT})\s*(?:-|−|–|—)\s*(\d{{1,2}})\s+({MONTH_ALT})(?:\s+(\d{{4}}))?(?![\dА-Яа-яЄєІіЇїҐґ])"
        ))
    });
    let text = sub_ctx(&text, &CROSS_MONTH_RANGE, |m, prefix| {
        let low =
            format!("{} {}", ordinal_words(parse_u64(cap(m, 1)), "gen"), month_name(cap(m, 2)));
        let high =
            format!("{} {}", ordinal_words(parse_u64(cap(m, 3)), "gen"), month_name(cap(m, 4)));
        let mut out = dates.range_connector(prefix, &low, &high);
        if matched(m, 5) {
            out.push_str(&format!(" {} року", ordinal_words(parse_u64(cap(m, 5)), "gen")));
        }
        out
    });

    let text = sub_ctx(&text, &DATE_DAY_RANGE_RE, |m, prefix| {
        let low = ordinal_words(parse_u64(cap(m, 1)), "gen");
        let high = ordinal_words(parse_u64(cap(m, 2)), "gen");
        format!(
            "{} {} {} року",
            dates.range_connector(prefix, &low, &high),
            month_name(cap(m, 3)),
            ordinal_words(parse_u64(cap(m, 4)), "gen")
        )
    });

    static DAY_RANGE_WITHOUT_YEAR: Lazy<Regex> = Lazy::new(|| {
        compile(&format!(
            r"\b(\d{{1,2}})\s*(?:-|−|–|—)\s*(\d{{1,2}})\s+({MONTH_ALT})(?!\s+\d{{2,4}})(?![А-Яа-яЄєІіЇїҐґ])"
        ))
    });
    let text = sub_ctx(&text, &DAY_RANGE_WITHOUT_YEAR, |m, prefix| {
        let first = parse_u64(cap(m, 1));
        let second = parse_u64(cap(m, 2));
        if !(1..=31).contains(&first) || !(1..=31).contains(&second) {
            return whole(m).to_owned();
        }
        let form = if range_style == RangeStyle::FromTo { "gen" } else { "nom_n" };
        let low = ordinal_words(first, form);
        let high = ordinal_words(second, form);
        format!("{} {}", dates.range_connector(prefix, &low, &high), month_name(cap(m, 3)))
    });

    static NUMERIC_RANGE: Lazy<Regex> = Lazy::new(|| {
        compile(r"\b(\d{1,2})\.(\d{1,2})\.(\d{4})\s*(?:-|−|–|—)\s*(\d{1,2})\.(\d{1,2})\.(\d{4})\b")
    });
    let text = sub(&text, &NUMERIC_RANGE, |m| {
        let form = if range_style == RangeStyle::FromTo { "gen" } else { "" };
        let first = dates.ordered_date_words(cap(m, 1), cap(m, 2), cap(m, 3), form);
        let second = dates.ordered_date_words(cap(m, 4), cap(m, 5), cap(m, 6), form);
        match (first, second) {
            (Some(first), Some(second)) => range_connector(range_style, &first, &second, false),
            _ => whole(m).to_owned(),
        }
    });

    static ISO_RANGE: Lazy<Regex> = Lazy::new(|| {
        compile(r"\b(\d{4})-(\d{2})-(\d{2})\s*(?:-|−|–|—)\s*(\d{4})-(\d{2})-(\d{2})\b")
    });
    let text = sub(&text, &ISO_RANGE, |m| {
        let m1 = parse_i32(cap(m, 2));
        let m2 = parse_i32(cap(m, 5));
        if !(1..=12).contains(&m1) || !(1..=12).contains(&m2) {
            return whole(m).to_owned();
        }
        if validate
            && (!is_valid_date(parse_i32(cap(m, 3)), m1, parse_i32(cap(m, 1)))
                || !is_valid_date(parse_i32(cap(m, 6)), m2, parse_i32(cap(m, 4))))
        {
            return whole(m).to_owned();
        }
        let low = format!(
            "{} {} {} року",
            dates.range_day_words(cap(m, 3)),
            MONTHS_GENITIVE[(m1 - 1) as usize],
            ordinal_words(parse_u64(cap(m, 1)), "gen")
        );
        let high = format!(
            "{} {} {} року",
            dates.range_day_words(cap(m, 6)),
            MONTHS_GENITIVE[(m2 - 1) as usize],
            ordinal_words(parse_u64(cap(m, 4)), "gen")
        );
        range_connector(range_style, &low, &high, false)
    });

    static GOVERNED_NUMERIC_DATE: Lazy<Regex> = Lazy::new(|| {
        compile_i(concat!(
            r"(^|[^А-Яа-яЄєІіЇїҐґ])(від|до|з|із|після|станом\s+на)\s+",
            r"(\d{1,2})[./-](\d{1,2})[./-](\d{2}|\d{4})\b(?:\s+(?:року|р\.))?"
        ))
    });
    let text = sub(&text, &GOVERNED_NUMERIC_DATE, |m| {
        let out = if whole(m).contains('/') {
            dates.slash_date_words(cap(m, 3), cap(m, 4), cap(m, 5), "gen")
        } else {
            dates.ordered_date_words(cap(m, 3), cap(m, 4), cap(m, 5), "gen")
        };
        match out {
            Some(out) => format!("{}{} {out}", cap(m, 1), cap(m, 2)),
            None => whole(m).to_owned(),
        }
    });

    /// Applies a date reader and leaves the text alone when it declines.
    fn try_read<F>(text: &str, re: &Regex, read: F) -> String
    where
        F: Fn(&Captures<'_, str>) -> Option<String>,
    {
        sub(text, re, |m| read(m).unwrap_or_else(|| whole(m).to_owned()))
    }

    static DMY_DASH: Lazy<Regex> = Lazy::new(|| compile(r"\b(\d{1,2})-(\d{1,2})-(\d{2}|\d{4})\b"));
    let text = try_read(&text, &DMY_DASH, |m| {
        dates.ordered_date_words(cap(m, 1), cap(m, 2), cap(m, 3), "")
    });

    static DMY_SHORT_DOT: Lazy<Regex> =
        Lazy::new(|| compile(r"\b(\d{1,2})\.(\d{1,2})\.(\d{2})\b(?:\s+(?:року|р\.))?"));
    let text = try_read(&text, &DMY_SHORT_DOT, |m| {
        dates.ordered_date_words(cap(m, 1), cap(m, 2), cap(m, 3), "")
    });

    static DMY_SHORT_SLASH: Lazy<Regex> =
        Lazy::new(|| compile(r"\b(\d{1,2})/(\d{1,2})/(\d{2})\b(?!/\d)(?:\s+(?:року|р\.))?"));
    let text = try_read(&text, &DMY_SHORT_SLASH, |m| {
        dates.slash_date_words(cap(m, 1), cap(m, 2), cap(m, 3), "")
    });

    static YMD_SLASH: Lazy<Regex> =
        Lazy::new(|| compile(r"\b(\d{4})/(\d{1,2})/(\d{1,2})\b(?!/\d)(?:\s+(?:року|р\.))?"));
    let text =
        try_read(&text, &YMD_SLASH, |m| dates.full_date_words(cap(m, 3), cap(m, 2), cap(m, 1), ""));

    static DMY_DOT: Lazy<Regex> =
        Lazy::new(|| compile(r"\b(\d{1,2})\.(\d{1,2})\.(\d{4})\b(?:\s+(?:року|р\.))?"));
    let text = try_read(&text, &DMY_DOT, |m| {
        dates.ordered_date_words(cap(m, 1), cap(m, 2), cap(m, 3), "")
    });

    static DMY_SLASH: Lazy<Regex> =
        Lazy::new(|| compile(r"\b(\d{1,2})/(\d{1,2})/(\d{4})\b(?!/\d)(?:\s+(?:року|р\.))?"));
    let text = try_read(&text, &DMY_SLASH, |m| {
        dates.slash_date_words(cap(m, 1), cap(m, 2), cap(m, 3), "")
    });

    static ISO_DATE: Lazy<Regex> = Lazy::new(|| compile(r"\b(\d{4})-(\d{2})-(\d{2})\b"));
    let text = sub(&text, &ISO_DATE, |m| {
        let month = parse_i32(cap(m, 2));
        if !(1..=12).contains(&month) {
            return whole(m).to_owned();
        }
        if validate && !is_valid_date(parse_i32(cap(m, 3)), month, parse_i32(cap(m, 1))) {
            return whole(m).to_owned();
        }
        format!(
            "{} {} {} року",
            dates.day_words(cap(m, 3), "nom_n"),
            MONTHS_GENITIVE[(month - 1) as usize],
            ordinal_words(parse_u64(cap(m, 1)), "gen")
        )
    });

    static YEAR_MONTH: Lazy<Regex> = Lazy::new(|| compile(r"\b(\d{4})-(0?[1-9]|1[0-2])(?!-?\d)"));
    let text = sub(&text, &YEAR_MONTH, |m| {
        let month = parse_i32(cap(m, 2)) as usize;
        format!(
            "{} {} року",
            MONTHS_NOMINATIVE[month - 1],
            ordinal_words(parse_u64(cap(m, 1)), "gen")
        )
    });

    let text = sub(&text, &DATE_SPELLED_RE, |m| {
        format!(
            "{} {} {} року",
            ordinal_words(parse_u64(cap(m, 1)), "gen"),
            month_name(cap(m, 2)),
            ordinal_words(parse_u64(cap(m, 3)), "gen")
        )
    });

    static NAMED_WITHOUT_YEAR: Lazy<Regex> = Lazy::new(|| {
        compile_i(&format!(r"\b(\d{{1,2}})\s+({MONTH_ANY})(?!\s+\d{{2,4}})(?![А-Яа-яЄєІіЇїҐґ])"))
    });
    let text = sub(&text, &NAMED_WITHOUT_YEAR, |m| {
        let day = parse_u64(cap(m, 1));
        if !(1..=31).contains(&day) {
            return whole(m).to_owned();
        }
        format!("{} {}", ordinal_words(day, "gen"), month_name(cap(m, 2)))
    });

    static NAMED_MONTH_YEAR: Lazy<Regex> = Lazy::new(|| {
        compile_i(&format!(
            r"(^|[^А-Яа-яЄєІіЇїҐґ])({MONTH_ANY})\s+(\d{{4}})(?:\s+(?:року|р\.)(?![А-Яа-яЄєІіЇїҐґ]))?(?!\d)"
        ))
    });
    let text = sub(&text, &NAMED_MONTH_YEAR, |m| {
        #[rustfmt::skip]
        const LOCATIVE: [&str; 12] = [
            "січні", "лютому", "березні", "квітні", "травні", "червні", "липні", "серпні",
            "вересні", "жовтні", "листопаді", "грудні",
        ];
        let source = lower_text(cap(m, 2));
        let month = if MONTHS_GENITIVE.contains(&source.as_str()) {
            month_name(&source)
        } else if LOCATIVE.contains(&source.as_str()) {
            source.clone()
        } else {
            month_nominative(&source)
        };
        format!("{}{month} {} року", cap(m, 1), ordinal_words(parse_u64(cap(m, 3)), "gen"))
    });

    static ABBREVIATED_YEAR_CONTEXT: Lazy<Regex> = Lazy::new(|| {
        compile(
            r"(^|[^А-Яа-яЄєІіЇїҐґ])(У|у|В|в|До|до|Від|від|Після|після)\s+(\d{3,4})\s*(?:р\.|рік)(?![а-яіїєґ])",
        )
    });
    let text = sub(&text, &ABBREVIATED_YEAR_CONTEXT, |m| {
        let preposition = lower_text(cap(m, 2));
        let locative = preposition == "у" || preposition == "в";
        format!(
            "{}{} {}{}",
            cap(m, 1),
            cap(m, 2),
            ordinal_words(parse_u64(cap(m, 3)), if locative { "prep" } else { "gen" }),
            if locative { " році" } else { " року" }
        )
    });

    static DECADE_WITHOUT_SUFFIX: Lazy<Regex> = Lazy::new(|| {
        compile(r"(^|[^А-Яа-яЄєІіЇїҐґ])(У|у|В|в)\s+(\d{4})\s+роках(?![А-Яа-яЄєІіЇїҐґ])")
    });
    let text = sub(&text, &DECADE_WITHOUT_SUFFIX, |m| {
        format!(
            "{}{} {} роках",
            cap(m, 1),
            cap(m, 2),
            ordinal_words(parse_u64(cap(m, 3)), "loc_pl")
        )
    });

    static YEAR_WITH_WORD: Lazy<Regex> =
        Lazy::new(|| compile(r"(^|[^\d])(\d{3,4})\s+(році|року|роком|рік)(?![А-Яа-яЄєІіЇїҐґ])"));
    let text = sub(&text, &YEAR_WITH_WORD, |m| {
        let form = match cap(m, 3) {
            "рік" => "nom_m",
            "року" => "gen",
            "році" => "prep",
            _ => "ins",
        };
        format!("{}{} {}", cap(m, 1), ordinal_words(parse_u64(cap(m, 2)), form), cap(m, 3))
    });

    static ORDINAL_YEAR_SUFFIX: Lazy<Regex> =
        Lazy::new(|| compile(r"(^|[^\d])(\d{3,4})[-–—](го|му|й|м)(?![А-Яа-яЄєІіЇїҐґ])"));
    let text = sub(&text, &ORDINAL_YEAR_SUFFIX, |m| {
        let form = match cap(m, 3) {
            "го" => "gen",
            "му" => "dat",
            "й" => "nom_m",
            _ => "prep",
        };
        format!("{}{}", cap(m, 1), ordinal_words(parse_u64(cap(m, 2)), form))
    });

    static YEAR_ABBREVIATION: Lazy<Regex> =
        Lazy::new(|| compile(r"\b(\d{3,4})\s*р\.(?![а-яіїєґ])"));
    sub(&text, &YEAR_ABBREVIATION, |m| {
        format!("{} рік", ordinal_words(parse_u64(cap(m, 1)), "nom_m"))
    })
}

/// Reads year spans, seasons and decades mentioned in prose.
pub(crate) fn normalize_discourse_dates(text: &str) -> String {
    static EXPLICIT_YEAR_SPAN: Lazy<Regex> = Lazy::new(|| {
        compile(concat!(
            r"(^|[^А-Яа-яЄєІіЇїҐґ])((?:З|з|Із|із|Від|від))\s+(\d{4})\s+(?:по|до)\s+(\d{4})",
            r"\s*(?:рр?\.?|роки)?(?![\dА-Яа-яЄєІіЇїҐґ])"
        ))
    });
    static SEASON_YEAR: Lazy<Regex> = Lazy::new(|| {
        compile_i(
            r"(^|[^А-Яа-яЄєІіЇїҐґ])((?:весна|літо|осінь|зима))\s+(\d{3,4})(?![\dА-Яа-яЄєІіЇїҐґ])",
        )
    });
    static EARLY_DECADE: Lazy<Regex> = Lazy::new(|| {
        compile_i(concat!(
            r"(^|[^А-Яа-яЄєІіЇїҐґ])((?:на\s+початку|у\s+середині|в\s+середині|наприкінці",
            r"|у\s+кінці|в\s+кінці))\s+(\d{4})-х(?![А-Яа-яЄєІіЇїҐґ])"
        ))
    });

    let text = sub(text, &EXPLICIT_YEAR_SPAN, |m| {
        format!(
            "{}{} {} до {} року",
            cap(m, 1),
            cap(m, 2),
            ordinal_words(parse_u64(cap(m, 3)), "gen"),
            ordinal_words(parse_u64(cap(m, 4)), "gen")
        )
    });
    let text = sub(&text, &SEASON_YEAR, |m| {
        format!("{}{} {} року", cap(m, 1), cap(m, 2), ordinal_words(parse_u64(cap(m, 3)), "gen"))
    });
    sub(&text, &EARLY_DECADE, |m| {
        let year = parse_u64(cap(m, 3));
        if year == 2000 {
            return format!("{}{} двотисячних", cap(m, 1), cap(m, 2));
        }
        format!("{}{} {}", cap(m, 1), cap(m, 2), ordinal_words(year, "pl"))
    })
}

/// Reads chapter-and-verse references to books of the Bible.
pub(crate) fn normalize_biblical_references(text: &str) -> String {
    const BOOKS: &str =
        "(?:Ісая|Єзекіїл|Буття|Вихід|Левит|Числа|Повторення Закону|Псалми|Матвій|Марко|Лука|Іван)";
    static REFERENCE_GROUP: Lazy<Regex> =
        Lazy::new(|| compile(&format!(r"\(({BOOKS})\s+([^)]{{3,120}})\)")));
    static CHAPTER_VERSE: Lazy<Regex> =
        Lazy::new(|| compile(r"(^|[^\d])(\d{1,3}):(\d{1,3})(?!\d)"));
    static LABELLED_REFERENCE: Lazy<Regex> = Lazy::new(|| {
        compile(&format!(r"(^|[^А-Яа-яЄєІіЇїҐґ])({BOOKS})\s+(\d{{1,3}}):(\d{{1,3}})(?!\d)"))
    });

    let say_references = |body: &str| {
        sub(body, &CHAPTER_VERSE, |m| {
            format!(
                "{}розділ {}, вірш {}",
                cap(m, 1),
                number_to_words(parse_u64(cap(m, 2))),
                number_to_words(parse_u64(cap(m, 3)))
            )
        })
    };
    let text =
        sub(text, &REFERENCE_GROUP, |m| format!("({} {})", cap(m, 1), say_references(cap(m, 2))));
    sub(&text, &LABELLED_REFERENCE, |m| {
        format!(
            "{}{} розділ {}, вірш {}",
            cap(m, 1),
            cap(m, 2),
            number_to_words(parse_u64(cap(m, 3))),
            number_to_words(parse_u64(cap(m, 4)))
        )
    })
}
