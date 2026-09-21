//! Options controlling how text is normalized.

use std::collections::HashMap;

/// How a numeric range such as `5–7` is read.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RangeStyle {
    /// `п'ять сім` — the two bounds side by side.
    #[default]
    Compact,
    /// `від п'яти до семи`.
    FromTo,
}

/// How the digits of a phone number are grouped.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PhoneStyle {
    /// Read in the groups the number is written in.
    #[default]
    Grouped,
    /// Read one digit at a time.
    DigitByDigit,
}

/// Whether symbols such as `±` and `§` are spoken.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum SymbolStyle {
    /// Replace symbols with words.
    #[default]
    Expand,
    /// Leave symbols as they are.
    Preserve,
}

/// How a date is read.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DateStyle {
    /// `першого травня дві тисячі двадцять четвертого року`.
    #[default]
    Formal,
    /// A shorter conversational reading.
    Spoken,
}

/// How `12:30` is interpreted.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ColonStyle {
    /// Decide from the surrounding text.
    #[default]
    Contextual,
    /// Always a clock time.
    Clock,
    /// Always a ratio.
    Ratio,
}

/// How `01.05.2024` orders its fields.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NumericDateOrder {
    /// Day first, the Ukrainian convention.
    #[default]
    DayMonthYear,
    /// Month first, the US convention.
    MonthDayYear,
    /// Leave dates alone when both fields could be a month.
    PreserveAmbiguous,
}

/// What to do with `$` and `¥`, which several currencies share.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CurrencySymbolPolicy {
    /// Read them as the most common currency (US dollar, Japanese yen).
    #[default]
    AssumeCommon,
    /// Leave the symbol in place.
    PreserveAmbiguous,
}

/// What to do with quotation marks.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum QuoteStyle {
    /// Leave them as written.
    #[default]
    Keep,
    /// Convert to `«»`.
    Guillemets,
    /// Convert to `"`.
    Straight,
    /// Remove them.
    Strip,
}

/// How forgiving lexicon lookups are toward a noisy, ASR-produced input.
///
/// Speech recognition often hands the normalizer a token that is *almost* a
/// known word — a missing apostrophe, a glued or split spelling, a surzhyk or
/// phonetic variant. `Strict` (the default) keeps the exact-match behaviour the
/// crate has always had. `Asr` adds a fallback that only runs *after* an exact
/// lookup misses: it folds the token to a canonical key and, failing that,
/// takes the closest lexicon entry within a bounded edit distance. Every such
/// fallback is reported by [`flag_uncertain_with`](super::flag_uncertain_with)
/// so the reading is never silently guessed.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum InputTolerance {
    /// Match lexicon keys exactly (backward-compatible default).
    #[default]
    Strict,
    /// Tolerate ASR distortions via a canonical-key and fuzzy fallback.
    Asr,
}

/// A named bundle of options for a common use case.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NormalizePreset {
    /// Balanced defaults.
    #[default]
    Default,
    /// Fully spelled out, for speech synthesis.
    TtsFriendly,
    /// Changes as little as possible.
    Conservative,
    /// Keeps tokens searchable rather than speakable.
    SearchIndexing,
}

/// Everything that steers [`normalize_with`](super::normalize_with).
///
/// [`NormalizeOptions::default()`] is the `Default` preset. Build another with
/// [`NormalizeOptions::preset`] and adjust the fields you care about:
///
/// ```
/// use normalize_uk::uktextnorm::{NormalizeOptions, NormalizePreset, RangeStyle};
///
/// let options = NormalizeOptions {
///     range_style: RangeStyle::Compact,
///     ..NormalizeOptions::preset(NormalizePreset::TtsFriendly)
/// };
/// assert_eq!(options.range_style, RangeStyle::Compact);
/// ```
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct NormalizeOptions {
    /// Expand acronyms listed in the lexicon (`ПДВ` -> `податок на додану вартість`).
    pub expand_known_acronyms: bool,
    /// Spell out acronyms that are not in the lexicon, letter by letter.
    pub spell_unknown_acronyms: bool,
    /// Give known English words a Ukrainian reading.
    pub normalize_english_words: bool,
    /// Transliterate any remaining Latin text into Cyrillic.
    pub transliterate_latin: bool,
    /// Repair Latin letters standing in for visually identical Cyrillic ones.
    pub repair_homoglyphs: bool,
    /// Leave impossible dates untouched instead of reading them.
    pub validate_dates: bool,
    /// Treat `1 000` and `1,000` as grouped thousands.
    pub parse_thousand_separators: bool,
    /// Read IP addresses, ports and similar network values.
    pub normalize_network_addresses: bool,
    /// What to do with quotation marks.
    pub quote_style: QuoteStyle,
    /// How ranges are read.
    pub range_style: RangeStyle,
    /// How phone numbers are grouped.
    pub phone_style: PhoneStyle,
    /// Whether symbols are spoken.
    pub symbol_style: SymbolStyle,
    /// How dates are read.
    pub date_style: DateStyle,
    /// How a colon between numbers is interpreted.
    pub colon_style: ColonStyle,
    /// How numeric dates order their fields.
    pub numeric_date_order: NumericDateOrder,
    /// What to do with currency symbols shared by several currencies.
    pub currency_symbol_policy: CurrencySymbolPolicy,
    /// How forgiving lexicon lookups are toward noisy, ASR-produced input.
    pub input_tolerance: InputTolerance,
    /// Extra canonical Cyrillic words the ASR fallback may repair a distorted
    /// token to, beyond the built-in closed sets.
    ///
    /// This is the extension point for tolerating distorted *ordinary* words:
    /// the built-in targets are only foreign-shaped closed sets (brand readings,
    /// acronyms), because fuzzy-matching open prose against itself would corrupt
    /// it. A caller that has a domain word list (medical terms, product names,
    /// a full Ukrainian lexicon) supplies it here, and — only under
    /// [`InputTolerance::Asr`] — a distorted token is folded to the closest
    /// entry by the same phonetic-key and bounded-edit rules. Empty by default.
    pub asr_vocabulary: Vec<String>,
    /// Lowercase Latin word to preferred Ukrainian reading. Entries here
    /// override the built-in brand and English-word lexicons.
    pub vocabulary: HashMap<String, String>,
}

impl Default for NormalizeOptions {
    fn default() -> Self {
        Self {
            expand_known_acronyms: true,
            spell_unknown_acronyms: true,
            normalize_english_words: true,
            transliterate_latin: true,
            repair_homoglyphs: true,
            validate_dates: true,
            parse_thousand_separators: true,
            normalize_network_addresses: true,
            quote_style: QuoteStyle::default(),
            range_style: RangeStyle::default(),
            phone_style: PhoneStyle::default(),
            symbol_style: SymbolStyle::default(),
            date_style: DateStyle::default(),
            colon_style: ColonStyle::default(),
            numeric_date_order: NumericDateOrder::default(),
            currency_symbol_policy: CurrencySymbolPolicy::default(),
            input_tolerance: InputTolerance::default(),
            asr_vocabulary: Vec::new(),
            vocabulary: HashMap::new(),
        }
    }
}

impl NormalizeOptions {
    /// The options a named preset stands for.
    #[must_use]
    pub fn preset(preset: NormalizePreset) -> Self {
        let base = Self::default();
        match preset {
            NormalizePreset::Default => base,
            NormalizePreset::TtsFriendly => Self {
                range_style: RangeStyle::FromTo,
                phone_style: PhoneStyle::DigitByDigit,
                date_style: DateStyle::Spoken,
                quote_style: QuoteStyle::Strip,
                ..base
            },
            NormalizePreset::Conservative => Self {
                repair_homoglyphs: false,
                expand_known_acronyms: false,
                spell_unknown_acronyms: false,
                normalize_english_words: false,
                transliterate_latin: false,
                symbol_style: SymbolStyle::Preserve,
                ..base
            },
            NormalizePreset::SearchIndexing => Self {
                quote_style: QuoteStyle::Straight,
                spell_unknown_acronyms: false,
                normalize_english_words: false,
                transliterate_latin: false,
                symbol_style: SymbolStyle::Preserve,
                ..base
            },
        }
    }
}

impl From<NormalizePreset> for NormalizeOptions {
    fn from(preset: NormalizePreset) -> Self {
        Self::preset(preset)
    }
}
