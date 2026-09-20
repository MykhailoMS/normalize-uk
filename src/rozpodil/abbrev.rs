//! Abbreviation tables consulted when deciding whether a period ends a sentence.

use once_cell::sync::Lazy;
use std::collections::HashSet;

fn set(values: &[&'static str]) -> HashSet<&'static str> {
    values.iter().copied().collect()
}

/// Abbreviations that normally follow the value they qualify (`5 тис.`).
#[rustfmt::skip]
pub(crate) static TRAILING: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    set(&[
        "тис", "млн", "млрд", "грн", "коп", "проц", "га", "кг", "г", "т", "куб", "кв", "км", "м",
        "см", "мм", "л", "год", "хв", "сек", "ст", "р", "рр", "с", "к", "руб", "крб", "co", "corp",
        "inc", "ed", "al", "мон", "моз", "мвс", "сбу", "нбу", "дпс", "дбр", "набу", "назк", "ова",
        "ода", "рда", "кмда", "мкм", "нм", "квт", "мвт", "шт", "од", "екз", "вс", "оаск", "єрдр",
        "ecli",
    ])
});

/// Abbreviations that normally precede what they qualify (`вул. Шевченка`).
#[rustfmt::skip]
pub(crate) static LEADING: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    set(&[
        "ст", "укр", "англ", "нім", "фр", "італ", "грец", "лат", "mr", "mrs", "ms", "dr", "vs",
        "св", "проф", "акад", "доц", "канд", "д-р", "ред", "гр", "ім", "тов", "п", "пп", "ч", "чч",
        "гл", "абз", "пт", "no", "просп", "пр", "вул", "ш", "м", "смт", "с", "обл", "р-н", "корп",
        "пер", "пл", "буд", "кв", "оф", "каб", "літ", "р", "а", "оз", "г", "напр", "дод", "юр",
        "фіз", "тел", "тобто", "див", "розд", "табл", "мал", "рис", "пор", "упоряд", "мкр", "наб",
        "пров", "шос", "бул", "деп", "пост", "наказ", "підп", "арк", "вип", "стор", "ухв", "ріш",
        "провадж", "спр", "поз", "позов", "відп", "заявн", "оск", "адмін", "крим", "цив", "госп",
        "док", "прим", "перекл", "вид", "т", "тт", "зб", "зош", "журн", "газ", "асист", "викл",
        "зав", "лаб", "інж", "н", "чл.-кор", "м-н", "ж/м", "в/ч", "остр", "річ", "станц", "залізн",
        "бл", "прибл", "зокр", "порівн", "підрозд", "тр", "скаржн", "кк", "кпк", "цк", "цпк", "гк",
        "гпк", "кас", "купап", "кзпп", "пку", "мку", "зку", "ску", "вс", "вп", "кцс", "кгс", "ккс",
        "оаск", "v", "єрдр", "ecli",
    ])
});

#[rustfmt::skip]
static OTHER: Lazy<HashSet<&'static str>> =
    Lazy::new(|| set(&["скор", "рис", "винят", "прим", "заст", "жарт"]));

/// Abbreviations that are really initials, so a period never ends the sentence.
#[rustfmt::skip]
pub(crate) static INITIALS: Lazy<HashSet<&'static str>> = Lazy::new(|| set(&["дж", "ed"]));

/// Two-word abbreviations where the period always joins (`і т. д.`).
#[rustfmt::skip]
pub(crate) static LEADING_PAIRS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    set(&["т е", "т к", "т н", "и о", "к н", "к п", "п н", "к т", "л д", "і т", "ст ст", "а с"])
});

/// Two-word abbreviations where the period joins if the next token can follow one.
#[rustfmt::skip]
pub(crate) static PAIRS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    set(&[
        "т п", "т д", "у е", "н э", "p m", "a m", "с г", "р х", "с ш", "з д", "л с", "ч т", "т е",
        "т к", "т н", "и о", "к н", "к п", "п н", "к т", "л д", "ед ч", "мн ч", "повел накл",
        "жен рмуж р", "і т", "ст ст", "а с",
    ])
});

/// True when `value` is any known abbreviation.
pub(crate) fn is_known(value: &str) -> bool {
    TRAILING.contains(value) || LEADING.contains(value) || OTHER.contains(value)
}
