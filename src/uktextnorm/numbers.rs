//! Turning numbers into Ukrainian words.

use super::lexicon::Forms;
use super::morphology::{
    feminine_last, inflect_ordinal, neuter_last, plural, CARDINAL_TO_ORDINAL, CASE_FORMS,
};
use super::text::{join, split_words, try_parse_u64};

#[rustfmt::skip]
const UNITS: [&str; 10] =
    ["", "один", "два", "три", "чотири", "п'ять", "шість", "сім", "вісім", "дев'ять"];
#[rustfmt::skip]
const TEENS: [&str; 10] = [
    "десять", "одинадцять", "дванадцять", "тринадцять", "чотирнадцять", "п'ятнадцять",
    "шістнадцять", "сімнадцять", "вісімнадцять", "дев'ятнадцять",
];
#[rustfmt::skip]
const TENS: [&str; 10] = [
    "", "десять", "двадцять", "тридцять", "сорок", "п'ятдесят", "шістдесят", "сімдесят",
    "вісімдесят", "дев'яносто",
];
#[rustfmt::skip]
const HUNDREDS: [&str; 10] = [
    "", "сто", "двісті", "триста", "чотириста", "п'ятсот", "шістсот", "сімсот", "вісімсот",
    "дев'ятсот",
];
#[rustfmt::skip]
const DIGIT_WORDS: [&str; 10] =
    ["нуль", "один", "два", "три", "чотири", "п'ять", "шість", "сім", "вісім", "дев'ять"];

/// The largest value [`number_to_words`] spells out rather than reading digit by digit.
pub const MAX_SPELLED_NUMBER: u64 = 999_999_999_999_999_999;

/// The words for a number below 1000, as separate tokens. Zero yields nothing.
pub(crate) fn under_thousand(n: u32) -> Vec<String> {
    match n {
        0 => Vec::new(),
        1..=9 => vec![UNITS[n as usize].to_owned()],
        10..=19 => vec![TEENS[(n - 10) as usize].to_owned()],
        20..=99 => {
            let mut out = vec![TENS[(n / 10) as usize].to_owned()];
            out.extend(under_thousand(n % 10));
            out
        }
        _ => {
            let mut out = vec![HUNDREDS[(n / 100) as usize].to_owned()];
            out.extend(under_thousand(n % 100));
            out
        }
    }
}

/// Reads each ASCII digit of `digits` separately, ignoring anything else.
///
/// ```
/// # use normalize_uk::uktextnorm::number_to_words_digit_by_digit;
/// assert_eq!(number_to_words_digit_by_digit("007"), "нуль нуль сім");
/// ```
pub fn number_to_words_digit_by_digit(digits: &str) -> String {
    digits
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|b| DIGIT_WORDS[(b - b'0') as usize])
        .collect::<Vec<_>>()
        .join(" ")
}

/// Spells `n` out in Ukrainian words.
///
/// Values above [`MAX_SPELLED_NUMBER`] are read digit by digit instead.
///
/// ```
/// # use normalize_uk::uktextnorm::number_to_words;
/// assert_eq!(number_to_words(0), "нуль");
/// assert_eq!(number_to_words(1_002), "тисяча два");
/// assert_eq!(number_to_words(2_002), "дві тисячі два");
/// ```
pub fn number_to_words(n: u64) -> String {
    if n == 0 {
        return "нуль".to_owned();
    }
    if n > MAX_SPELLED_NUMBER {
        return number_to_words_digit_by_digit(&n.to_string());
    }
    /// A scale word, its forms, and whether it takes a feminine number.
    struct Scale(u64, Forms, bool);
    let scales = [
        Scale(
            1_000_000_000_000_000,
            Forms {
                one: "квадрильйон", few: "квадрильйони", many: "квадрильйонів"
            },
            false,
        ),
        Scale(
            1_000_000_000_000,
            Forms {
                one: "трильйон", few: "трильйони", many: "трильйонів"
            },
            false,
        ),
        Scale(1_000_000_000, Forms { one: "мільярд", few: "мільярди", many: "мільярдів" }, false),
        Scale(1_000_000, Forms { one: "мільйон", few: "мільйони", many: "мільйонів" }, false),
        Scale(1_000, Forms { one: "тисяча", few: "тисячі", many: "тисяч" }, true),
    ];

    let mut words: Vec<String> = Vec::new();
    for Scale(value, forms, feminine) in &scales {
        let count = ((n / value) % 1000) as u32;
        if count == 0 {
            continue;
        }
        let mut chunk = under_thousand(count);
        if *feminine {
            feminine_last(&mut chunk);
            // "одна тисяча" drops the "одна" only when it is the whole chunk.
            if count == 1 {
                chunk.pop();
            }
        }
        words.extend(chunk);
        words.push(plural(count as u64, forms).to_owned());
    }
    words.extend(under_thousand((n % 1000) as u32));
    join(&words)
}

/// Grammatical forms an ordinal can take.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
#[allow(missing_docs)]
pub enum OrdinalForm {
    /// Masculine nominative, the dictionary form.
    #[default]
    NomM,
    NomN,
    NomF,
    NomPl,
    Gen,
    Dat,
    Prep,
    Loc,
    Pl,
    LocPl,
    AccF,
    GenF,
    Ins,
    InsF,
    InsPl,
    LocF,
}

impl OrdinalForm {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::NomM => "nom_m",
            Self::NomN => "nom_n",
            Self::NomF => "nom_f",
            Self::NomPl => "nom_pl",
            Self::Gen => "gen",
            Self::Dat => "dat",
            Self::Prep => "prep",
            Self::Loc => "loc",
            Self::Pl => "pl",
            Self::LocPl => "loc_pl",
            Self::AccF => "acc_f",
            Self::GenF => "gen_f",
            Self::Ins => "ins",
            Self::InsF => "ins_f",
            Self::InsPl => "ins_pl",
            Self::LocF => "loc_f",
        }
    }
}

/// Grammatical cases a cardinal number can be put into.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(missing_docs)]
pub enum GrammaticalCase {
    Genitive,
    Dative,
    Instrumental,
    Prepositional,
}

impl GrammaticalCase {
    const fn index(self) -> usize {
        match self {
            Self::Genitive => 0,
            Self::Dative => 1,
            Self::Instrumental => 2,
            Self::Prepositional => 3,
        }
    }
}

/// Spells `n` out as an ordinal in the requested form.
///
/// ```
/// # use normalize_uk::uktextnorm::{number_to_ordinal_words, OrdinalForm};
/// assert_eq!(number_to_ordinal_words(1, OrdinalForm::NomM), "перший");
/// assert_eq!(number_to_ordinal_words(3, OrdinalForm::Gen), "третього");
/// ```
pub fn number_to_ordinal_words(n: u64, form: OrdinalForm) -> String {
    ordinal_words(n, form.as_str())
}

pub(crate) fn ordinal_words(n: u64, form: &str) -> String {
    // Round thousands have a dedicated stem: 3000 -> трьохтисячний.
    if (2000..=9000).contains(&n) && n % 1000 == 0 {
        const PREFIXES: [&str; 10] =
            ["", "", "двох", "трьох", "чотирьох", "п'яти", "шести", "семи", "восьми", "дев'яти"];
        return inflect_ordinal(&format!("{}тисячний", PREFIXES[(n / 1000) as usize]), form);
    }
    let mut words = split_words(&number_to_words(n));
    let Some(last) = words.last_mut() else {
        return String::new();
    };
    let stem = CARDINAL_TO_ORDINAL.get(last.as_str()).copied().unwrap_or(last.as_str());
    *last = inflect_ordinal(stem, form);
    join(&words)
}

/// Spells `n` out in the requested grammatical case.
///
/// ```
/// # use normalize_uk::uktextnorm::{number_to_words_case, GrammaticalCase};
/// assert_eq!(number_to_words_case(2, GrammaticalCase::Genitive), "двох");
/// ```
pub fn number_to_words_case(n: u64, case: GrammaticalCase) -> String {
    let index = case.index();
    let words: Vec<String> = split_words(&number_to_words(n))
        .into_iter()
        .map(|w| CASE_FORMS.get(w.as_str()).map_or(w.clone(), |forms| forms[index].to_owned()))
        .collect();
    join(&words)
}

/// [`number_to_words_case`] keyed by the case names used internally.
pub(crate) fn number_to_words_case_str(n: u64, case: &str) -> String {
    let case = match case {
        "gen" => GrammaticalCase::Genitive,
        "dat" => GrammaticalCase::Dative,
        "instr" => GrammaticalCase::Instrumental,
        _ => GrammaticalCase::Prepositional,
    };
    number_to_words_case(n, case)
}

/// The words for `n`, with the last word agreeing with the given gender.
pub(crate) fn number_words_for_gender(n: u64, gender: char) -> String {
    let mut words = split_words(&number_to_words(n));
    match gender {
        'f' => feminine_last(&mut words),
        'n' => neuter_last(&mut words),
        _ => {}
    }
    join(&words)
}

/// The words for `n` in the given case and gender, as separate tokens.
pub(crate) fn number_words_for_case(n: u64, case: &str, gender: char) -> Vec<String> {
    let mut words = split_words(&number_to_words(n));
    match gender {
        'f' => feminine_last(&mut words),
        'n' => neuter_last(&mut words),
        _ => {}
    }
    if case == "gen" {
        for word in &mut words {
            if let Some(forms) = CASE_FORMS.get(word.as_str()) {
                *word = forms[0].to_owned();
            }
        }
    }
    words
}

/// True when a genitive number takes the "many" noun form (`п'ять днів`).
pub(crate) fn prefers_many_after_genitive_number(n: u64) -> bool {
    let (mod100, mod10) = (n % 100, n % 10);
    mod10 == 0 || mod10 >= 5 || (11..=14).contains(&mod100)
}

/// The words for a number, falling back to digit-by-digit when it does not fit `u64`.
pub(crate) fn number_digits_or_words(digits: &str) -> String {
    match try_parse_u64(digits) {
        Some(value) => number_to_words(value),
        None => number_to_words_digit_by_digit(digits),
    }
}

/// Fractional place names in the nominative, indexed by fraction digit count.
#[rustfmt::skip]
const PLACES_NOM: [(&str, &str); 6] = [
    ("десята", "десятих"),
    ("сота", "сотих"),
    ("тисячна", "тисячних"),
    ("десятитисячна", "десятитисячних"),
    ("стотисячна", "стотисячних"),
    ("мільйонна", "мільйонних"),
];

/// The same place names in the genitive.
#[rustfmt::skip]
const PLACES_GEN: [(&str, &str); 6] = [
    ("десятої", "десятих"),
    ("сотої", "сотих"),
    ("тисячної", "тисячних"),
    ("десятитисячної", "десятитисячних"),
    ("стотисячної", "стотисячних"),
    ("мільйонної", "мільйонних"),
];

fn place(
    places: &[(&'static str, &'static str); 6],
    digits: usize,
) -> Option<(&'static str, &'static str)> {
    (1..=places.len()).contains(&digits).then(|| places[digits - 1])
}

/// Reads `int_part.frac_part` as a proper fraction, or `None` when the fraction
/// has too many digits to name.
pub(crate) fn decimal_to_words(int_part: &str, frac_part: &str) -> Option<String> {
    let (singular, plural_form) = place(&PLACES_NOM, frac_part.len())?;
    let int_value = try_parse_u64(int_part)?;
    let frac_value = try_parse_u64(frac_part)?;
    let mut int_words = split_words(&number_to_words(int_value));
    feminine_last(&mut int_words);
    let whole =
        if int_value % 10 == 1 && int_value % 100 != 11 { "ціла" } else { "цілих" };
    let mut frac_words = split_words(&number_to_words(frac_value));
    feminine_last(&mut frac_words);
    let name = if frac_value % 10 == 1 && frac_value % 100 != 11 { singular } else { plural_form };
    Some(format!("{} {whole} і {} {name}", join(&int_words), join(&frac_words)))
}

/// Like [`decimal_to_words`], but falls back to `<int> кома <digits>`.
pub(crate) fn decimal_to_words_or_digits(int_part: &str, frac_part: &str) -> String {
    if let Some(words) = decimal_to_words(int_part, frac_part) {
        return words;
    }
    let int_part = if int_part.is_empty() { "0" } else { int_part };
    format!(
        "{} кома {}",
        number_digits_or_words(int_part),
        number_to_words_digit_by_digit(frac_part)
    )
}

/// Strips a leading sign from `token` and returns how it is spoken.
pub(crate) fn take_spoken_sign(token: &mut &str) -> &'static str {
    for minus in ["-", "−", "–", "—"] {
        if let Some(rest) = token.strip_prefix(minus) {
            *token = rest;
            return "мінус ";
        }
    }
    if let Some(rest) = token.strip_prefix('+') {
        *token = rest;
        return "плюс ";
    }
    ""
}

/// Reads a possibly signed, possibly decimal number in the given case and gender.
pub(crate) fn signed_number_words(token: &str, case: &str, gender: char) -> Option<String> {
    let mut token = token;
    let sign = take_spoken_sign(&mut token);
    let Some(pos) = token.find(['.', ',']) else {
        let value = try_parse_u64(token)?;
        return Some(format!("{sign}{}", join(&number_words_for_case(value, case, gender))));
    };

    let (int_part, frac_part) = (&token[..pos], &token[pos + 1..]);
    let integer = if int_part.is_empty() { Some(0) } else { try_parse_u64(int_part) };
    let (integer, fraction) = (integer?, try_parse_u64(frac_part)?);
    let (singular, plural_form) = place(&PLACES_GEN, frac_part.len())?;
    if case != "gen" {
        return decimal_to_words(int_part, frac_part).map(|words| format!("{sign}{words}"));
    }
    let whole = if integer % 10 == 1 && integer % 100 != 11 {
        " цілої і "
    } else {
        " цілих і "
    };
    let name = if fraction % 10 == 1 && fraction % 100 != 11 { singular } else { plural_form };
    Some(format!(
        "{sign}{}{whole}{} {name}",
        join(&number_words_for_case(integer, "gen", 'f')),
        join(&number_words_for_case(fraction, "gen", 'f')),
    ))
}

/// `13` -> `тринадцята година`.
pub(crate) fn hours_words(hour: u64) -> String {
    let mut words = split_words(&number_to_words(hour));
    feminine_last(&mut words);
    format!(
        "{} {}",
        join(&words),
        plural(hour, &Forms { one: "година", few: "години", many: "годин" })
    )
}

/// `30` -> `тридцять хвилин`, with the unit forms configurable.
pub(crate) fn minutes_words(minute: u64, forms: &Forms) -> String {
    let mut words = split_words(&number_to_words(minute));
    feminine_last(&mut words);
    format!("{} {}", join(&words), plural(minute, forms))
}

pub(crate) const MINUTE_FORMS: Forms =
    Forms { one: "хвилина", few: "хвилини", many: "хвилин" };
pub(crate) const SECOND_FORMS: Forms =
    Forms { one: "секунда", few: "секунди", many: "секунд" };

/// `3/4` -> `три четвертих`.
pub(crate) fn say_fraction(num: u64, den: u64) -> String {
    let mut words = split_words(&number_to_words(num));
    feminine_last(&mut words);
    let singular = num % 10 == 1 && num % 100 != 11;
    format!("{} {}", join(&words), ordinal_words(den, if singular { "nom_f" } else { "pl" }))
}
