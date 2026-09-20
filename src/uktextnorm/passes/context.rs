//! Agreement with the surrounding words: case after a preposition, counted
//! nouns, ordinal-triggering nouns and numeric compounds.

use fancy_regex::Regex;
use once_cell::sync::Lazy;
use std::collections::HashMap;

use crate::uktextnorm::lexicon::Gender;
use crate::uktextnorm::morphology::{plural, CASE_FORMS, COMPOUND_PREFIX_FORMS};
use crate::uktextnorm::numbers::{
    number_to_words, number_to_words_case_str, number_words_for_gender, ordinal_words,
    prefers_many_after_genitive_number,
};
use crate::uktextnorm::patterns::{
    CASE_PREP_RE, COUNTED_GENITIVE_RE, COUNTED_NOUNS_RE, COUNTED_PONAD_RE,
};
use crate::uktextnorm::re::{cap, compile, compile_i, matched, sub, sub_ctx, whole};
use crate::uktextnorm::readers::{COUNTED_NOUNS, COUNTED_OBLIQUE, MEASUREMENTS};
use crate::uktextnorm::text::{lower_text, parse_u64, regex_alternation, split_words};

use super::ranges::preceding_word;

fn gender_char(gender: Gender) -> char {
    match gender {
        Gender::Feminine => 'f',
        Gender::Neuter => 'n',
        Gender::Masculine => 'm',
    }
}

/// The case each preposition puts the following number into.
#[rustfmt::skip]
static PREPOSITION_CASE: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    [
        ("близько", "gen"), ("менше", "gen"), ("більше", "gen"), ("серед", "gen"),
        ("від", "gen"), ("до", "gen"), ("із", "gen"), ("з", "instr"), ("без", "gen"),
        ("після", "gen"), ("протягом", "gen"), ("впродовж", "gen"), ("упродовж", "gen"),
        ("перед", "instr"), ("між", "instr"), ("над", "instr"), ("під", "instr"),
        ("при", "prep"), ("к", "dat"), ("о", "prep"), ("об", "prep"),
    ]
    .into_iter()
    .collect()
});

/// Puts numbers into the case the governing preposition requires.
pub(crate) fn normalize_case_context(text: &str) -> String {
    static PONAD_QUANTITY: Lazy<Regex> =
        Lazy::new(|| compile(r"(^|[^А-Яа-яЄєІіЇїҐґ])(Понад|понад)\s+(\d+)(?!\d)"));
    static QUANTIFIED_GENITIVE: Lazy<Regex> =
        Lazy::new(|| compile(r"(^|[\s(\[{:;,.!?])((?:З|з))\s+(\d+)\s+([^\s,.;:!?]+)"));
    static COMPARATIVE_GENITIVE: Lazy<Regex> = Lazy::new(|| {
        compile(
            r"(^|[^А-Яа-яЄєІіЇїҐґ])(Після|після|До|до|Від|від|Без|без)\s+(більш|менш)\s+ніж\s+(\d+)(?!\d)",
        )
    });
    static INSTRUMENTAL: Lazy<Regex> = Lazy::new(|| {
        compile(
            r"(^|[^А-Яа-яЄєІіЇїҐґ])([Зз])\s+(\d+)\s+([а-яєіїґ']{3,}(?:ами|ями|ма))(?![А-Яа-яЄєІіЇїҐґ])",
        )
    });
    static BEFORE_LOCATIVE_ADJECTIVE: Lazy<Regex> =
        Lazy::new(|| compile(r"(^|[\s(\[{:;,.!?])(У|у|В|в|На|на)\s+(\d+)\s+([^\s,.;:!?]+)"));
    static OBLIQUE: Lazy<Regex> = Lazy::new(|| {
        compile_i(&format!(
            r"(^|[^А-Яа-яЄєІіЇїҐґ])(У|у|В|в|На|на)\s+(\d+)\s+({})(?![А-Яа-яЄєІіЇїҐґ])",
            regex_alternation(COUNTED_OBLIQUE.keys().copied())
        ))
    });

    let text = sub(text, &PONAD_QUANTITY, |m| {
        let mut words = number_to_words(parse_u64(cap(m, 3)));
        // "понад тисячу", not "понад тисяча".
        if let Some(rest) = words.strip_prefix("тисяча ") {
            words = format!("тисячу {rest}");
        }
        format!("{}{} {words}", cap(m, 1), cap(m, 2))
    });
    let text = sub(&text, &QUANTIFIED_GENITIVE, |m| {
        let noun = lower_text(cap(m, 4));
        if !noun.ends_with("ів") && !noun.ends_with("їв") {
            return whole(m).to_owned();
        }
        format!(
            "{}{} {} {}",
            cap(m, 1),
            cap(m, 2),
            number_to_words_case_str(parse_u64(cap(m, 3)), "gen"),
            cap(m, 4)
        )
    });
    let text = sub(&text, &COMPARATIVE_GENITIVE, |m| {
        format!(
            "{}{} {} ніж {}",
            cap(m, 1),
            cap(m, 2),
            cap(m, 3),
            number_to_words_case_str(parse_u64(cap(m, 4)), "gen")
        )
    });
    let text = sub(&text, &INSTRUMENTAL, |m| {
        format!(
            "{}{} {} {}",
            cap(m, 1),
            cap(m, 2),
            number_to_words_case_str(parse_u64(cap(m, 3)), "instr"),
            cap(m, 4)
        )
    });
    let text = sub(&text, &BEFORE_LOCATIVE_ADJECTIVE, |m| {
        let count = parse_u64(cap(m, 3));
        let adjective = lower_text(cap(m, 4));
        let locative = ["ому", "ьому", "ій", "их"].iter().any(|s| adjective.ends_with(s));
        if count == 1 || !locative {
            return whole(m).to_owned();
        }
        format!(
            "{}{} {} {}",
            cap(m, 1),
            cap(m, 2),
            number_to_words_case_str(count, "prep"),
            cap(m, 4)
        )
    });
    let text = sub(&text, &CASE_PREP_RE, |m| {
        let preposition = lower_text(cap(m, 2));
        let Some(&case) = PREPOSITION_CASE.get(preposition.as_str()) else {
            return whole(m).to_owned();
        };
        // A unit only agrees when the preposition takes the genitive.
        if matched(m, 4) && case != "gen" {
            return whole(m).to_owned();
        }
        let mut out = format!(
            "{}{} {}",
            cap(m, 1),
            cap(m, 2),
            number_to_words_case_str(parse_u64(cap(m, 3)), case)
        );
        if matched(m, 4) {
            let Some(unit) = MEASUREMENTS.get(cap(m, 4)) else {
                return whole(m).to_owned();
            };
            out.push_str(&format!(" {}{}", unit.forms.many, cap(m, 5)));
        }
        out
    });
    sub(&text, &OBLIQUE, |m| {
        let noun = cap(m, 4);
        match COUNTED_OBLIQUE.get(lower_text(noun).as_str()) {
            Some(case) => format!(
                "{}{} {} {noun}",
                cap(m, 1),
                cap(m, 2),
                number_to_words_case_str(parse_u64(cap(m, 3)), case)
            ),
            None => whole(m).to_owned(),
        }
    })
}

/// Agrees a counted noun with the number in front of it, after a preposition.
pub(crate) fn normalize_counted_noun_context(text: &str) -> String {
    let text = sub(text, &COUNTED_PONAD_RE, |m| {
        let n = parse_u64(cap(m, 3));
        let Some(noun) = COUNTED_NOUNS.get(lower_text(cap(m, 4)).as_str()) else {
            return whole(m).to_owned();
        };
        format!(
            "{}{} {} {}",
            cap(m, 1),
            cap(m, 2),
            number_words_for_gender(n, gender_char(noun.gender)),
            plural(n, &noun.forms)
        )
    });
    sub(&text, &COUNTED_GENITIVE_RE, |m| {
        let n = parse_u64(cap(m, 3));
        let Some(noun) = COUNTED_NOUNS.get(lower_text(cap(m, 4)).as_str()) else {
            return whole(m).to_owned();
        };
        if !prefers_many_after_genitive_number(n) {
            return whole(m).to_owned();
        }
        format!(
            "{}{} {} {}",
            cap(m, 1),
            cap(m, 2),
            number_to_words_case_str(n, "gen"),
            noun.forms.many
        )
    })
}

/// Agrees a bare counted noun with the number in front of it.
pub(crate) fn normalize_counted_nouns(text: &str) -> String {
    sub_ctx(text, &COUNTED_NOUNS_RE, |m, prefix| {
        let n = parse_u64(cap(m, 2));
        let Some(noun) = COUNTED_NOUNS.get(lower_text(cap(m, 3)).as_str()) else {
            return whole(m).to_owned();
        };
        // "частина 2 статті" is a reference, not a count of articles.
        if noun.forms.one == "стаття" {
            let previous = preceding_word(prefix);
            if matches!(previous.as_str(), "частина" | "пункт" | "розділ" | "параграф" | "глава")
            {
                return whole(m).to_owned();
            }
        }
        format!(
            "{}{} {}",
            cap(m, 1),
            number_words_for_gender(n, gender_char(noun.gender)),
            plural(n, &noun.forms)
        )
    })
}

/// Reads a number as an ordinal when the noun after it calls for one.
pub(crate) fn normalize_ordinal_triggers(text: &str) -> String {
    static GENITIVE_CLASS: Lazy<Regex> =
        Lazy::new(|| compile_i(r"(^|[^\dА-Яа-яЄєІіЇїҐґ])(\d{1,2})\s+(класу)(?![А-Яа-яЄєІіЇїҐґ])"));
    #[rustfmt::skip]
    static TRIGGERS: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
        [
            ("місце", "nom_n"), ("село", "nom_n"), ("місто", "nom_n"), ("століття", "nom_n"),
            ("ст", "nom_n"), ("клас", "nom_m"), ("курс", "nom_m"), ("раунд", "nom_m"),
            ("сезон", "nom_m"), ("етап", "nom_m"), ("тур", "nom_m"), ("том", "nom_m"),
            ("під'їзд", "nom_m"), ("поверх", "nom_m"), ("група", "nom_f"),
            ("квартира", "nom_f"), ("сторінка", "nom_f"),
        ]
        .into_iter()
        .collect()
    });
    static RE: Lazy<Regex> = Lazy::new(|| {
        compile_i(concat!(
            r"(^|[^\d])(\d{1,4})\s+(місце|село|місто|століття|ст|клас|курс|раунд|сезон|етап",
            r"|тур|том|під'їзд|поверх|група|квартира|сторінка)(?![А-Яа-яЄєІіЇїҐґ])"
        ))
    });

    let text = sub(text, &GENITIVE_CLASS, |m| {
        let number = parse_u64(cap(m, 2));
        if number == 0 {
            return whole(m).to_owned();
        }
        format!("{}{} {}", cap(m, 1), ordinal_words(number, "gen"), cap(m, 3))
    });
    sub(&text, &RE, |m| {
        let noun = lower_text(cap(m, 3));
        match TRIGGERS.get(noun.as_str()) {
            Some(form) => {
                format!("{}{} {}", cap(m, 1), ordinal_words(parse_u64(cap(m, 2)), form), cap(m, 3))
            }
            None => whole(m).to_owned(),
        }
    })
}

/// Turns `5-поверховий` into `п'ятиповерховий`.
pub(crate) fn normalize_compounds(text: &str) -> String {
    static RE: Lazy<Regex> = Lazy::new(|| {
        compile(concat!(
            r"(^|[^\d])(\d+)-(?!(?:ший|ими|им|ім|ою|ій|ше|ша|ге|га|тє|тя|го|му|й|м|а|у|е|х)",
            r"(?:[^А-Яа-яЄєІіЇїҐґ]|$))([^0-9A-Za-z\s,.;:!?()]+)"
        ))
    });
    sub(text, &RE, |m| {
        let mut prefix = String::new();
        for word in split_words(&number_to_words(parse_u64(cap(m, 2)))) {
            if let Some(form) = COMPOUND_PREFIX_FORMS.get(word.as_str()) {
                prefix.push_str(form);
            } else if let Some(forms) = CASE_FORMS.get(word.as_str()) {
                prefix.push_str(forms[0]);
            } else {
                prefix.push_str(&word);
            }
        }
        format!("{}{prefix}{}", cap(m, 1), cap(m, 3))
    })
}
