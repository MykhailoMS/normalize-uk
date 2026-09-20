//! Ordinals, Roman numerals, centuries, quarters, page and section ranges.

use fancy_regex::Regex;
use once_cell::sync::Lazy;
use std::collections::{HashMap, HashSet};

use crate::uktextnorm::lexicon::Forms;
use crate::uktextnorm::morphology::plural;
use crate::uktextnorm::numbers::{
    number_to_words, number_to_words_case_str, number_words_for_gender, ordinal_words,
};
use crate::uktextnorm::re::{cap, compile, compile_i, matched, sub, sub_around, sub_ctx, whole};
use crate::uktextnorm::readers::read_dotted;
use crate::uktextnorm::text::{lower_text, parse_u64, try_parse_u64};
use crate::uktextnorm::validation::{roman_to_int, valid_roman};
use crate::uktextnorm::RangeStyle;

/// The grammatical form each ordinal suffix stands for.
#[rustfmt::skip]
static SUFFIX_FORM: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    [
        ("й", "nom_m"), ("ший", "nom_m"), ("го", "gen"), ("му", "dat"), ("м", "prep"),
        ("а", "nom_f"), ("ша", "nom_f"), ("га", "nom_f"), ("тя", "nom_f"), ("у", "acc_f"),
        ("е", "nom_n"), ("ше", "nom_n"), ("ге", "nom_n"), ("тє", "nom_n"), ("х", "pl"),
        ("им", "ins"), ("ім", "ins"), ("ою", "ins_f"), ("ій", "loc_f"), ("ими", "ins_pl"),
    ]
    .into_iter()
    .collect()
});

/// Acronyms that look like Roman numerals but never are.
#[rustfmt::skip]
static STOP_WORDS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    ["CD", "DVD", "MD", "DC", "MC", "MI", "MM", "DI", "DIV", "DVI", "DL", "CLI", "MIX", "CIV", "LCD"]
        .into_iter()
        .collect()
});

/// Ukrainian Wikipedia commonly writes Roman centuries with Cyrillic
/// homoglyphs (ХХІ). Repair only in a century context, never in identifiers.
fn cyrillic_roman_value(token: &str) -> Option<u64> {
    if !token.contains('Х') && !token.contains('І') {
        return None;
    }
    let latin = token.replace('Х', "X").replace('І', "I");
    valid_roman(&latin).then(|| roman_to_int(&latin))
}

/// The last word of `text`, lowercased, ignoring trailing whitespace.
fn trailing_word(text: &str) -> String {
    let lowered = lower_text(text);
    let trimmed = lowered.trim_end();
    match trimmed.rfind([' ', '\t', '\n']) {
        Some(boundary) => trimmed[boundary + 1..].to_owned(),
        None => trimmed.to_owned(),
    }
}

/// Reads ordinal suffixes, Roman numerals, centuries and Roman group labels.
pub(crate) fn normalize_ordinals(text: &str) -> String {
    const ROMAN_OR_CYRILLIC: &str = r"(?:Х|І|X|I|V|M|C|D|L){1,8}";
    const CENTURY_NOUN: &str = r"(ст\.|століття|столітті|сторіччя|сторіччі)";

    static CYRILLIC_CENTURY_RANGE: Lazy<Regex> = Lazy::new(|| {
        compile(&format!(
            r"(^|[^A-Za-zА-Яа-яЄєІіЇїҐґ])({ROMAN_OR_CYRILLIC})\s*(?:-|–|—)\s*({ROMAN_OR_CYRILLIC})\s*{CENTURY_NOUN}(?![А-Яа-яЄєІіЇїҐґ])"
        ))
    });
    let text = sub_ctx(text, &CYRILLIC_CENTURY_RANGE, |m, prefix| {
        let (Some(first), Some(second)) =
            (cyrillic_roman_value(cap(m, 2)), cyrillic_roman_value(cap(m, 3)))
        else {
            return whole(m).to_owned();
        };
        let word = trailing_word(&format!("{prefix}{}", cap(m, 1)));
        let noun = cap(m, 4);
        let is_sorichchia = noun.starts_with("сторіч");
        if word == "у" || word == "в" || noun == "столітті" || noun == "сторіччі"
        {
            return format!(
                "{}{}–{}{}",
                cap(m, 1),
                ordinal_words(first, "prep"),
                ordinal_words(second, "prep"),
                if is_sorichchia { " сторіччях" } else { " століттях" }
            );
        }
        format!(
            "{}від {} до {} {}",
            cap(m, 1),
            ordinal_words(first, "gen"),
            ordinal_words(second, "gen"),
            if is_sorichchia { "сторіччя" } else { "століття" }
        )
    });

    static CYRILLIC_CENTURY: Lazy<Regex> = Lazy::new(|| {
        compile(&format!(
            r"(^|[^A-Za-zА-Яа-яЄєІіЇїҐґ])({ROMAN_OR_CYRILLIC})\s*{CENTURY_NOUN}(?![А-Яа-яЄєІіЇїҐґ])"
        ))
    });
    let text = sub_ctx(&text, &CYRILLIC_CENTURY, |m, prefix| {
        let Some(value) = cyrillic_roman_value(cap(m, 2)) else {
            return whole(m).to_owned();
        };
        let word = trailing_word(&format!("{prefix}{}", cap(m, 1)));
        let noun = cap(m, 3);
        let locative = word == "у" || word == "в" || noun == "столітті" || noun == "сторіччі";
        let genitive = matches!(word.as_str(), "початку" | "кінця" | "середини" | "половини");
        let form = if locative {
            "prep"
        } else if genitive {
            "gen"
        } else {
            "nom_n"
        };
        let noun = if noun.starts_with("сторіч") {
            if locative {
                " сторіччі"
            } else {
                " сторіччя"
            }
        } else if locative {
            " столітті"
        } else {
            " століття"
        };
        format!("{}{}{noun}", cap(m, 1), ordinal_words(value, form))
    });

    static BARE_CENTURY_BEFORE_START: Lazy<Regex> = Lazy::new(|| {
        compile(&format!(
            r"(Протягом|протягом|Впродовж|впродовж|Упродовж|упродовж)\s+({ROMAN_OR_CYRILLIC})\s+та\s+початку"
        ))
    });
    let text = sub(&text, &BARE_CENTURY_BEFORE_START, |m| match cyrillic_roman_value(cap(m, 2)) {
        Some(value) => {
            format!("{} {} століття та початку", cap(m, 1), ordinal_words(value, "gen"))
        }
        None => whole(m).to_owned(),
    });

    static ORDINAL_SUFFIX: Lazy<Regex> = Lazy::new(|| {
        compile(concat!(
            r"(\d+)(?:-|–|—)(ший|ими|им|ім|ою|ій|ше|ша|ге|га|тє|тя|го|му|й|м|а|у|е|х)",
            r"(?![А-Яа-яЄєІіЇїҐґ])"
        ))
    });
    let text = sub(&text, &ORDINAL_SUFFIX, |m| match SUFFIX_FORM.get(cap(m, 2)) {
        Some(form) => ordinal_words(parse_u64(cap(m, 1)), form),
        None => whole(m).to_owned(),
    });

    /// Reads a pair of Roman numerals, or leaves the match alone.
    fn roman_pair(m: &fancy_regex::Captures<'_, str>, form: &str, noun: &str) -> String {
        let (start, stop) = (cap(m, 2), cap(m, 3));
        if !valid_roman(start) || !valid_roman(stop) {
            return whole(m).to_owned();
        }
        format!(
            "{}{} {} {noun}",
            cap(m, 1),
            ordinal_words(roman_to_int(start), form),
            ordinal_words(roman_to_int(stop), form)
        )
    }

    static ROMAN_CENTURY_RANGE: Lazy<Regex> = Lazy::new(|| {
        compile(
            r"(^|[^A-Za-z])([MDCLXVI]{1,6})\s*(?:-|–|—)\s*([MDCLXVI]{1,6})\s*(?:ст\.|століття)(?![А-Яа-яЄєІіЇїҐґ])",
        )
    });
    let text = sub(&text, &ROMAN_CENTURY_RANGE, |m| roman_pair(m, "nom_n", "століття"));

    static ROMAN_SECTION_RANGE: Lazy<Regex> = Lazy::new(|| {
        compile(
            r"(^|[^A-Za-z])([MDCLXVI]{1,6})\s*(?:-|–|—)\s*([MDCLXVI]{1,6})\s*(розд\.|розділ)(?![А-Яа-яЄєІіЇїҐґ])",
        )
    });
    let text = sub(&text, &ROMAN_SECTION_RANGE, |m| roman_pair(m, "nom_m", "розділ"));

    static ROMAN_CENTURY: Lazy<Regex> = Lazy::new(|| {
        compile(r"(^|[^A-Za-z])([MDCLXVI]{1,6})\s*(?:ст\.|століття)(?![А-Яа-яЄєІіЇїҐґ])")
    });
    let text = sub(&text, &ROMAN_CENTURY, |m| {
        let token = cap(m, 2);
        if !valid_roman(token) {
            return whole(m).to_owned();
        }
        format!("{}{} століття", cap(m, 1), ordinal_words(roman_to_int(token), "nom_n"))
    });

    static ROMAN_GROUP: Lazy<Regex> = Lazy::new(|| {
        compile_i(r"(^|[^A-Za-z])([MDCLXVI]{1,6})\s+(група|групи)(?![А-Яа-яЄєІіЇїҐґ])")
    });
    let text = sub(&text, &ROMAN_GROUP, |m| {
        let token = cap(m, 2);
        if !valid_roman(token) {
            return whole(m).to_owned();
        }
        let form = if lower_text(cap(m, 3)) == "групи" { "gen_f" } else { "nom_f" };
        format!("{}{} {}", cap(m, 1), ordinal_words(roman_to_int(token), form), cap(m, 3))
    });

    static BARE_ROMAN: Lazy<Regex> = Lazy::new(|| compile(r"\b[MDCLXVI]{2,}\b"));
    sub(&text, &BARE_ROMAN, |m| {
        let token = whole(m);
        if STOP_WORDS.contains(token) || !valid_roman(token) {
            return token.to_owned();
        }
        ordinal_words(roman_to_int(token), "nom_m")
    })
}

/// Reads calendar quarters written with a Roman or Arabic numeral.
pub(crate) fn normalize_quarters(text: &str) -> String {
    static ROMAN_QUARTER: Lazy<Regex> = Lazy::new(|| {
        compile(concat!(
            r"(^|[^A-Za-z])([MDCLXVI]{1,6})\s*(?:кв\.|квартал)",
            r"(?:\s+(\d{3,4})(?:\s*р\.|\s+року)?)?(?![А-Яа-яЄєІіЇїҐґ])"
        ))
    });
    static NUMERIC_QUARTER: Lazy<Regex> = Lazy::new(|| {
        compile(concat!(
            r"(^|[^\d])(\d{1,2})(?:[-–—]?(?:й|ій))?\s*(?:кв\.|квартал)",
            r"(?:\s+(\d{3,4})(?:\s*р\.|\s+року)?)?(?![А-Яа-яЄєІіЇїҐґ])"
        ))
    });

    fn say(quarter: u64, year: Option<&str>) -> Option<String> {
        if !(1..=4).contains(&quarter) {
            return None;
        }
        let mut out = format!("{} квартал", ordinal_words(quarter, "nom_m"));
        if let Some(year) = year {
            out.push_str(&format!(" {} року", ordinal_words(parse_u64(year), "gen")));
        }
        Some(out)
    }

    let text = sub(text, &ROMAN_QUARTER, |m| {
        let token = cap(m, 2);
        if !valid_roman(token) {
            return whole(m).to_owned();
        }
        let year = matched(m, 3).then(|| cap(m, 3));
        match say(roman_to_int(token), year) {
            Some(out) => format!("{}{out}", cap(m, 1)),
            None => whole(m).to_owned(),
        }
    });
    sub(&text, &NUMERIC_QUARTER, |m| {
        let year = matched(m, 3).then(|| cap(m, 3));
        match say(parse_u64(cap(m, 2)), year) {
            Some(out) => format!("{}{out}", cap(m, 1)),
            None => whole(m).to_owned(),
        }
    })
}

/// Reads page counts, page numbers and page ranges in bibliographies.
pub(crate) fn normalize_page_ranges(text: &str, style: RangeStyle) -> String {
    static BIBLIOGRAPHIC_VOLUMES: Lazy<Regex> =
        Lazy::new(|| compile(r"(^|[\s,:;])(У|у|В|в)\s+(\d+)\s+(?:т|Т)(?:т|Т)?\.?([ \t]*)(?=/)"));
    static PAGE_COUNT: Lazy<Regex> =
        Lazy::new(|| compile_i(r"(^|(?:-|—)\s+)(\d+)\s+(?:с|С)\.(?=\s*(?::|;|-|—|ISBN|$))"));
    static PAGE_RANGE: Lazy<Regex> = Lazy::new(|| {
        compile_i(concat!(
            r"(^|[^А-Яа-яЄєІіЇїҐґA-Za-z])(?:стор\.|Стор\.|СТОР\.|с\.|С\.|pp?\.)",
            r"\s*(\d+)\s*(?:-|−|–|—)\s*(\d+)(?!\d)"
        ))
    });
    static SINGLE_PAGE: Lazy<Regex> = Lazy::new(|| {
        compile_i(r"(^|[^А-Яа-яЄєІіЇїҐґA-Za-z])(?:стор|Стор|СТОР|с|С|pp?)\.\s*(\d+)(?!\d)")
    });

    let text = sub(text, &BIBLIOGRAPHIC_VOLUMES, |m| {
        let count = parse_u64(cap(m, 3));
        let singular = count % 10 == 1 && count % 100 != 11;
        format!(
            "{}{} {}{}{}",
            cap(m, 1),
            cap(m, 2),
            number_to_words_case_str(count, "prep"),
            if singular { " томі" } else { " томах" },
            cap(m, 4)
        )
    });
    let text = sub_around(&text, &PAGE_COUNT, |m, ctx| {
        let count = parse_u64(cap(m, 2));
        format!(
            "{}{} {}{}",
            cap(m, 1),
            number_words_for_gender(count, 'f'),
            plural(
                count,
                &Forms {
                    one: "сторінка", few: "сторінки", many: "сторінок"
                }
            ),
            // A page count that ends the text keeps its full stop.
            if ctx.suffix.is_empty() { "." } else { "" }
        )
    });
    let text = sub(&text, &PAGE_RANGE, |m| {
        let (Some(low), Some(high)) = (try_parse_u64(cap(m, 2)), try_parse_u64(cap(m, 3))) else {
            return whole(m).to_owned();
        };
        if style == RangeStyle::FromTo {
            format!(
                "{}від {} до {} сторінки",
                cap(m, 1),
                ordinal_words(low, "gen_f"),
                ordinal_words(high, "gen_f")
            )
        } else {
            format!("{}сторінки {} {}", cap(m, 1), number_to_words(low), number_to_words(high))
        }
    });
    sub(&text, &SINGLE_PAGE, |m| {
        format!("{}сторінка {}", cap(m, 1), number_to_words(parse_u64(cap(m, 2))))
    })
}

/// How one kind of section reference is read as a range.
struct SectionRange {
    /// The plural used in the compact style.
    compact: &'static str,
    /// The genitive used after "від ... до".
    genitive: &'static str,
    ordinal_form: &'static str,
}

#[rustfmt::skip]
static SECTIONS: Lazy<HashMap<&'static str, SectionRange>> = Lazy::new(|| {
    let gen_f = |compact| SectionRange { compact, genitive: compact, ordinal_form: "gen_f" };
    [
        ("ст", gen_f("статті")),
        ("статті", gen_f("статті")),
        ("ч", gen_f("частини")),
        ("частини", gen_f("частини")),
        ("п", SectionRange { compact: "пункти", genitive: "пункту", ordinal_form: "gen" }),
        ("пункти", SectionRange { compact: "пункти", genitive: "пункту", ordinal_form: "gen" }),
        ("пп", SectionRange { compact: "підпункти", genitive: "підпункту", ordinal_form: "gen" }),
        ("підпункти", SectionRange { compact: "підпункти", genitive: "підпункту", ordinal_form: "gen" }),
        ("абз", SectionRange { compact: "абзаци", genitive: "абзацу", ordinal_form: "gen" }),
        ("розд", SectionRange { compact: "розділи", genitive: "розділу", ordinal_form: "gen" }),
        ("гл", gen_f("глави")),
        ("табл", gen_f("таблиці")),
        ("рис", SectionRange { compact: "рисунки", genitive: "рисунка", ordinal_form: "gen" }),
    ]
    .into_iter()
    .collect()
});

/// Reads ranges of articles, clauses, chapters, tables and figures.
pub(crate) fn normalize_section_ranges(text: &str, style: RangeStyle) -> String {
    static DOTTED_RANGE: Lazy<Regex> = Lazy::new(|| {
        compile_i(concat!(
            r"(^|[^А-Яа-яЄєІіЇїҐґA-Za-z])(пп|підпункти)\.?\s*(\d+(?:\.\d+)+)",
            r"\s*(?:-|−|–|—)\s*(\d+(?:\.\d+)+)(?![\d.])"
        ))
    });
    static RANGE: Lazy<Regex> = Lazy::new(|| {
        compile_i(concat!(
            r"(^|[^А-Яа-яЄєІіЇїҐґA-Za-z])(ст|статті|ч|частини|пп|підпункти|п|пункти|абз|розд|гл|табл|рис)",
            r"\.?\s*(\d+)\s*(?:-|−|–|—)\s*(\d+)(?!\d)"
        ))
    });

    let text = sub(text, &DOTTED_RANGE, |m| {
        if style == RangeStyle::FromTo {
            format!(
                "{}від підпункту {} до підпункту {}",
                cap(m, 1),
                read_dotted(cap(m, 3)),
                read_dotted(cap(m, 4))
            )
        } else {
            format!("{}підпункти {} {}", cap(m, 1), read_dotted(cap(m, 3)), read_dotted(cap(m, 4)))
        }
    });
    sub(&text, &RANGE, |m| {
        let (Some(low), Some(high)) = (try_parse_u64(cap(m, 3)), try_parse_u64(cap(m, 4))) else {
            return whole(m).to_owned();
        };
        let Some(section) = SECTIONS.get(lower_text(cap(m, 2)).as_str()) else {
            return whole(m).to_owned();
        };
        if style == RangeStyle::FromTo {
            format!(
                "{}від {} до {} {}",
                cap(m, 1),
                ordinal_words(low, section.ordinal_form),
                ordinal_words(high, section.ordinal_form),
                section.genitive
            )
        } else {
            format!(
                "{}{} {} {}",
                cap(m, 1),
                section.compact,
                number_to_words(low),
                number_to_words(high)
            )
        }
    })
}
