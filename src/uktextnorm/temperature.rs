//! Temperature scales and how a quantity on one is read.

use super::lexicon::Forms;
use super::morphology::plural;
use super::numbers::{signed_number_words, take_spoken_sign};
use super::text::{lower_text, try_parse_u64};
use std::sync::LazyLock;

/// The temperature scales the normalizer recognizes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ScaleKind {
    Celsius,
    Fahrenheit,
    Kelvin,
    Rankine,
    Reaumur,
    Delisle,
    Newton,
    Romer,
}

/// A scale together with how its unit is named.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Scale {
    pub kind: ScaleKind,
    /// The scale's name in the genitive, or empty for kelvin.
    pub genitive_name: &'static str,
    pub unit_forms: Forms,
    /// The form used after a decimal quantity.
    pub decimal_unit: &'static str,
}

const DEGREE_FORMS: Forms =
    Forms { one: "градус", few: "градуси", many: "градусів" };

const fn degrees(kind: ScaleKind, genitive_name: &'static str) -> Scale {
    Scale { kind, genitive_name, unit_forms: DEGREE_FORMS, decimal_unit: "градуса" }
}

const CELSIUS: Scale = degrees(ScaleKind::Celsius, "Цельсія");
const FAHRENHEIT: Scale = degrees(ScaleKind::Fahrenheit, "Фаренгейта");
const RANKINE: Scale = degrees(ScaleKind::Rankine, "Ранкіна");
const REAUMUR: Scale = degrees(ScaleKind::Reaumur, "Реомюра");
const DELISLE: Scale = degrees(ScaleKind::Delisle, "Деліля");
const NEWTON: Scale = degrees(ScaleKind::Newton, "Ньютона");
const ROMER: Scale = degrees(ScaleKind::Romer, "Ремера");
const KELVIN: Scale = Scale {
    kind: ScaleKind::Kelvin,
    genitive_name: "",
    unit_forms: Forms {
        one: "кельвін", few: "кельвіни", many: "кельвінів"
    },
    decimal_unit: "кельвіна",
};

fn compact_lower(text: &str) -> String {
    lower_text(text).chars().filter(|c| !c.is_whitespace()).collect()
}

/// Identifies the scale a unit token names, if any.
///
/// The checks run from the most specific scale to the least, because several
/// abbreviations share a leading letter (`°R` is Rankine, `°Re` is Réaumur).
pub(crate) fn temperature_scale(scale: &str) -> Option<Scale> {
    let lowered = lower_text(scale);
    let compact = compact_lower(scale);
    let named_degrees = lowered.contains("град");
    let ends_any = |suffixes: &[&str]| suffixes.iter().any(|s| compact.ends_with(s));
    let is = |values: &[&str]| values.contains(&compact.as_str());

    // The bare "K" here is U+212A KELVIN SIGN; lowercasing leaves it unchanged.
    if lowered.contains("кельв")
        || is(&["k", "к", "\u{212a}", "°k", "°к"])
        || (named_degrees && ends_any(&["k", "к"]))
    {
        return Some(KELVIN);
    }
    if lowered.contains("реомюр")
        || is(&["°re", "°ré", "°rÉ"])
        || (named_degrees && ends_any(&["re", "ré", "rÉ"]))
    {
        return Some(REAUMUR);
    }
    if lowered.contains("деліл") || compact == "°de" || (named_degrees && compact.ends_with("de"))
    {
        return Some(DELISLE);
    }
    if lowered.contains("ньютон") || compact == "°n" {
        return Some(NEWTON);
    }
    if lowered.contains("ремер")
        || is(&["°rø", "°rØ", "°rō", "°rŌ"])
        || (named_degrees && ends_any(&["rø", "rØ", "rō", "rŌ"]))
    {
        return Some(ROMER);
    }
    if lowered.contains("ранкін") || is(&["°r", "°ra"]) || (named_degrees && ends_any(&["r", "ra"]))
    {
        return Some(RANKINE);
    }
    if lowered.contains("фаренгейт")
        || is(&["f", "°f", "℉"])
        || (named_degrees && compact.ends_with('f'))
    {
        return Some(FAHRENHEIT);
    }
    if lowered.contains("цельс")
        || lowered.contains("celsius")
        || is(&["c", "с", "°c", "°с", "℃"])
        || (named_degrees && ends_any(&["c", "с"]))
    {
        return Some(CELSIUS);
    }
    None
}

/// A regex fragment matching any way a temperature unit can be written.
pub(crate) static TEMPERATURE_UNIT_PATTERN: LazyLock<String> = LazyLock::new(|| {
    const DEGREE_SYMBOL: &str = concat!(
        r"(?:°\s*(?:Celsius|celsius|Fahrenheit|fahrenheit|C|c|С|с|F|f|N|n|D(?:e|E)|d[eE]",
        r"|R(?:a|A|e|E|é|É|ø|Ø|ō|Ō)?|r(?:a|e|é|ø|ō)?)|℃|℉)"
    );
    const KELVIN_SYMBOL: &str = "(?:K|\u{041a}|\u{212a}|°\\s*(?:K|k|\u{041a}|\u{043a}))";
    const DEGREES: &str =
        r"(?:град\.?|Град\.?|ГРАД\.?|градус(?:а|и|ів)?|Градус(?:а|и|ів)?|ГРАДУС(?:А|И|ІВ)?)";
    const DEGREE_SCALE: &str = concat!(
        r"(?:C|c|С|с|F|f|R|r|Ra|ra|Re|re|Ré|ré|De|de|Rø|rø|Rō|rō|Цельсія|цельсія|ЦЕЛЬСІЯ",
        r"|Фаренгейта|фаренгейта|ФАРЕНГЕЙТА|Ранкіна|ранкіна|РАНКІНА|Реомюра|реомюра|РЕОМЮРА",
        r"|Деліля|деліля|ДЕЛІЛЯ|Ньютона|ньютона|НЬЮТОНА|Ремера|ремера|РЕМЕРА",
        r"|за\s+(?:Цельсієм|цельсієм|ЦЕЛЬСІЄМ|Фаренгейтом|фаренгейтом|ФАРЕНГЕЙТОМ|Ранкіном",
        r"|ранкіном|РАНКІНОМ|Реомюром|реомюром|РЕОМЮРОМ|Делілем|делілем|ДЕЛІЛЕМ|Ньютоном",
        r"|ньютоном|НЬЮТОНОМ|Ремером|ремером|РЕМЕРОМ))"
    );
    const KELVIN_NAME: &str = r"(?:кельвін(?:а|и|ів)?|Кельвін(?:а|и|ів)?|КЕЛЬВІН(?:А|И|ІВ)?)";
    const KELVIN_DEGREE_NAME: &str = concat!(
        "(?:K|k|\u{041a}|\u{043a}|\u{212a}|Кельвіна|кельвіна|КЕЛЬВІНА",
        "|за\\s+(?:Кельвіном|кельвіном|КЕЛЬВІНОМ))"
    );
    format!(
        r"(?:{DEGREE_SYMBOL}|(?:C|c|С|с|F|f)(?!\.)|{KELVIN_NAME}|{KELVIN_SYMBOL}|{DEGREES}\s+(?:{DEGREE_SCALE}|{KELVIN_DEGREE_NAME}))"
    )
});

/// Reads a temperature quantity together with its unit.
pub(crate) fn temperature_quantity_words(
    token: &str,
    scale: &Scale,
    grammatical_case: &str,
) -> Option<String> {
    let words = signed_number_words(token, grammatical_case, 'm')?;
    let name = if scale.genitive_name.is_empty() {
        String::new()
    } else {
        format!(" {}", scale.genitive_name)
    };
    if token.contains(['.', ',']) {
        return Some(format!("{words} {}{name}", scale.decimal_unit));
    }
    let mut unsigned = token;
    take_spoken_sign(&mut unsigned);
    let value = try_parse_u64(unsigned)?;
    let unit = if grammatical_case == "gen" {
        scale.unit_forms.many
    } else {
        plural(value, &scale.unit_forms)
    };
    Some(format!("{words} {unit}{name}"))
}

/// The unit form that agrees with the upper bound of a temperature range.
pub(crate) fn temperature_range_unit(upper: &str, scale: &Scale) -> &'static str {
    if upper.contains(['.', ',']) {
        return scale.decimal_unit;
    }
    let mut upper = upper;
    take_spoken_sign(&mut upper);
    match try_parse_u64(upper) {
        Some(value) if value % 10 == 1 && value % 100 != 11 => scale.decimal_unit,
        _ => scale.unit_forms.many,
    }
}
