//! The individual normalization passes.

mod abbreviations;
mod alphanumeric;
mod context;
mod currency;
mod dates;
mod identifiers;
mod ordinals;
pub(crate) mod ranges;
mod structural;
mod units;

pub use abbreviations::{expand_abbreviations, normalize_abbreviations, transliterate_to_cyrillic};

pub(crate) use alphanumeric::{
    canonicalize_asr, normalize_cyrillic_alphanumeric, normalize_english,
    normalize_technical_alphanumeric,
};
pub(crate) use context::{
    normalize_case_context, normalize_compounds, normalize_counted_noun_context,
    normalize_counted_nouns, normalize_ordinal_triggers,
};
pub(crate) use currency::{
    normalize_currency, normalize_finance, normalize_known_acronyms,
    normalize_regional_currency_aliases, normalize_symbol_currency,
};
pub(crate) use dates::{normalize_biblical_references, normalize_dates, normalize_discourse_dates};
pub(crate) use identifiers::{
    normalize_coordinates, normalize_identifiers, normalize_ip_addresses,
};
pub(crate) use ordinals::{
    normalize_ordinals, normalize_page_ranges, normalize_quarters, normalize_section_ranges,
};
pub(crate) use ranges::normalize_ranges;
pub(crate) use structural::{
    normalize_addresses, normalize_homoglyphs, normalize_number_groups, normalize_sections,
    normalize_symbols, normalize_text_with_phone_numbers, normalize_typography, normalize_unicode,
    normalize_web,
};
pub(crate) use units::{
    normalize_decimals, normalize_fractions, normalize_math, normalize_measurements,
    normalize_medical, normalize_multipliers, normalize_negatives,
    normalize_overprecise_currency_decimals, normalize_percent, normalize_scientific,
    normalize_text_with_numbers, normalize_time, normalize_versions,
};
