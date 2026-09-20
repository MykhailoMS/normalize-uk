//! Robustness checks: awkward input must never panic, lose the text entirely,
//! or produce spans that do not line up with the source.

use normalize_uk::uktextnorm::{flag_uncertain, normalize_preset, NormalizePreset};

#[rustfmt::skip]
const PRESETS: [NormalizePreset; 4] = [
    NormalizePreset::Default,
    NormalizePreset::TtsFriendly,
    NormalizePreset::Conservative,
    NormalizePreset::SearchIndexing,
];

/// Inputs chosen to stress the parsers: empty, oversized, malformed, mixed.
#[rustfmt::skip]
const AWKWARD: [(&str, &str); 19] = [
    ("empty", ""),
    ("spaces", " \t  \n "),
    ("oversized number", "Номер 1234567890123456789012345678901234567890"),
    ("oversized dotted", "Версія 999999999999999999999999.999999999999999999999999.1"),
    ("malformed date", "Дата 99.99.9999 і 2026-99-99"),
    ("malformed url email", "Контакти https:// test@ @ _"),
    ("mixed scripts", "FooКиїв BarЛьвів АAАA"),
    ("dangling signs", "Ціна ₴ $ € £ № + - / : ;"),
    ("overprecise money", "Сума 1,234567890123456789 грн і $999999999999999999999999.99"),
    ("long phone-like", "Телефон 380671234567890123456789 і 0671234567890"),
    ("legal soup", "ч. ст. п. розд. № -- 910//1234///24"),
    ("roman soup", "IIII ст. VX розд. XIX-INVALID"),
    ("unicode punctuation", "«Тест» – — … ½ ⅞ 50/0"),
    ("combining apostrophes", "П'ять зв’язків мʼясо ІМ`Я"),
    (
        "latin products",
        "OpenAI ChatGPT GitHub Kubernetes TypeScript v999999999999999999999.1",
    ),
    ("query string", "https://example.com/a?x=1&y=2&&&&"),
    ("compact finance", "BTC/UAH ETH/USD 000000000000000000000001 BTC"),
    (
        "measurement soup",
        "999999999999999999999999 кг 1,23456789 мг/мл -999999999999999999999999%",
    ),
    (
        "range soup",
        "999999999999999999999999–1000000000000000000000000 °C, -5,5–+7,25 кг, 10:30–12:45, 1/0–3/4",
    ),
];

#[test]
fn normalization_survives_awkward_input() {
    for (name, text) in AWKWARD {
        for preset in PRESETS {
            let normalized = normalize_preset(text, preset);
            assert!(
                text.is_empty() || !normalized.is_empty(),
                "{name} ({preset:?}): normalization emptied a non-empty input"
            );
        }
    }
}

#[test]
fn uncertainty_spans_stay_inside_the_source() {
    for (name, text) in AWKWARD {
        for span in flag_uncertain(text) {
            assert!(span.start <= span.stop, "{name}: span start is after its stop");
            assert!(span.stop <= text.len(), "{name}: span runs past the end of the source");
            assert_eq!(
                &text[span.start..span.stop],
                span.text,
                "{name}: span offsets do not match its text"
            );
        }
    }
}

/// Inputs whose normalized form must be a fixed point.
#[rustfmt::skip]
const IDEMPOTENT: [&str; 24] = [
    "5 кг",
    "2026-09",
    "10.0.0.0/24",
    "0.5 DOGE",
    "<speak>5 кг</speak>",
    "6.02×10²³",
    "ст. 5–7",
    "-1/2",
    "−1/2",
    "-2,5 м/с²",
    "PT1.5H",
    "[2001:db8::1]:443",
    "[5 кг](https://example.com/a_(b)?x=1)",
    "5‐7 °C",
    "$1,234.56",
    "3 N*m",
    "1.234 BHD",
    "2 AVAX",
    "BTC 2",
    "1,000 BTC",
    "1.000,25 ETH",
    "₿0.5",
    "0.25 NEWCOIN",
    "NEWCOIN/USDT",
];

#[test]
fn normalization_is_idempotent() {
    for text in IDEMPOTENT {
        let once = normalize_preset(text, NormalizePreset::TtsFriendly);
        let twice = normalize_preset(&once, NormalizePreset::TtsFriendly);
        assert_eq!(once, twice, "{text:?} normalized differently on the second pass");
    }
}
