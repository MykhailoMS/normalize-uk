//! Currency amounts, finance tickers and known acronyms.

use fancy_regex::Regex;
use once_cell::sync::Lazy;
use std::collections::HashMap;

use crate::uktextnorm::lexicon::{self, Currency};
use crate::uktextnorm::morphology::{feminine_last, plural};
use crate::uktextnorm::numbers::{decimal_to_words, number_to_words};
use crate::uktextnorm::patterns::{
    CURRENCY_CODE_ALT, CURRENCY_TOKEN_ALT, SYMBOL_CURRENCY_PREFIX_RE, SYMBOL_CURRENCY_SUFFIX_RE,
};
use crate::uktextnorm::re::{cap, compile, compile_i, sub, sub_ctx, whole};
use crate::uktextnorm::readers::{
    finance_amount_words, finance_amount_words_parts, spell_identifier_letters, FINANCE_UNITS,
};
use crate::uktextnorm::text::{
    capitalize_first_letter, join, lower_text, regex_alternation, split_words, try_parse_u64,
};

/// The non-breaking space used as a thousands separator in some sources.
const NBSP: char = '\u{a0}';

/// A number with an optional sign, optional grouping and up to four decimals.
const AMOUNT_TOKEN: &str = concat!(
    r"[+-]?(?:[1-9]\d{0,2}(?:,\d{3})+(?:\.\d{1,4})?",
    r"|[1-9]\d{0,2}(?:\.\d{3})+(?:,\d{1,4})?",
    r"|\d+[.,]\d{1,4}|\d+|[.,]\d{1,4})(?!\d|[.,]\d)"
);

/// A grouped or plain amount, as finance tickers write them.
const RAW_AMOUNT: &str = "(?:\\d{1,3}(?:[ ,.\u{a0}]\\d{3})+(?:[.,]\\d+)?|\\d+(?:[.,]\\d+)?)";

/// An uppercase token that could be a ticker, but not a bare number.
const GENERIC_TICKER: &str = r"(?![0-9]{2,10}\b)[A-Z0-9]{2,10}";

fn amount_group() -> String {
    format!("({AMOUNT_TOKEN})")
}

/// Escapes a currency symbol for use inside a pattern.
fn escape(symbol: &str) -> String {
    let mut out = String::with_capacity(symbol.len());
    for ch in symbol.chars() {
        if r"\-[]{}()*+?.,^$|# ".contains(ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Expands the acronyms listed in the lexicon, capitalizing the expansion when
/// the acronym opens a sentence.
pub(crate) fn normalize_known_acronyms(text: &str) -> String {
    static MAP: Lazy<HashMap<&'static str, &'static str>> =
        Lazy::new(|| lexicon::ACRONYMS.iter().copied().collect());
    static RE: Lazy<Regex> = Lazy::new(|| {
        compile(&format!(
            r"(^|[^А-Яа-яЄєІіЇїҐґ])((?:{})(?![А-Яа-яЄєІіЇїҐґ]))",
            regex_alternation(lexicon::ACRONYMS.iter().map(|&(a, _)| a))
        ))
    });
    sub_ctx(text, &RE, |m, prefix_text| {
        let Some(&expansion) = MAP.get(cap(m, 2)) else { return whole(m).to_owned() };
        let prefix = cap(m, 1);
        let before = format!("{prefix_text}{prefix}");
        let before = before.trim_end();
        let sentence_start = before.is_empty() || before.ends_with(['.', '!', '?']);
        let expansion =
            if sentence_start { capitalize_first_letter(expansion) } else { expansion.to_owned() };
        format!("{prefix}{expansion}")
    })
}

/// Reads `$5 млн` and `5 млн $` by moving the currency after the scale word.
pub(crate) fn normalize_symbol_currency(text: &str) -> String {
    static GENITIVE_PLURAL: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
        let mut out: HashMap<&str, &str> = HashMap::from([("грн", "гривень")]);
        for entry in lexicon::CURRENCIES.iter() {
            out.entry(entry.code).or_insert(entry.main.many);
            if !entry.symbol.is_empty() {
                out.entry(entry.symbol).or_insert(entry.main.many);
            }
        }
        out
    });
    let text = sub(text, &SYMBOL_CURRENCY_PREFIX_RE, |m| match GENITIVE_PLURAL.get(cap(m, 1)) {
        Some(name) => format!("{} {} {name}", cap(m, 2), cap(m, 3)),
        None => whole(m).to_owned(),
    });
    sub(&text, &SYMBOL_CURRENCY_SUFFIX_RE, |m| match GENITIVE_PLURAL.get(cap(m, 3)) {
        Some(name) => format!("{} {} {name}", cap(m, 1), cap(m, 2)),
        None => whole(m).to_owned(),
    })
}

/// Rewrites regional dollar and yen signs as their ISO code.
pub(crate) fn normalize_regional_currency_aliases(text: &str) -> String {
    #[rustfmt::skip]
    static ALIASES: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
        [
            ("us$", "USD"), ("ca$", "CAD"), ("au$", "AUD"), ("nz$", "NZD"), ("hk$", "HKD"),
            ("sg$", "SGD"), ("jp¥", "JPY"), ("cn¥", "CNY"), ("r$", "BRL"),
        ]
        .into_iter()
        .collect()
    });
    static RE: Lazy<Regex> =
        Lazy::new(|| compile_i(r"(US\$|CA\$|AU\$|NZ\$|HK\$|SG\$|JP¥|CN¥|R\$)"));
    sub(text, &RE, |m| {
        ALIASES
            .get(lower_text(whole(m)).as_str())
            .map_or_else(|| whole(m).to_owned(), |c| (*c).to_owned())
    })
}

/// Normalizes an amount's separators and reads it with the currency's forms.
///
/// Returns `None` when the separators cannot be a valid amount for this
/// currency, in which case the caller leaves the text alone.
fn amount_words(raw: &str, currency: &Currency) -> Option<String> {
    let mut amount = raw.replace(' ', "");
    let sign = match amount.chars().next() {
        Some('-') => {
            amount.remove(0);
            "мінус "
        }
        Some('+') => {
            amount.remove(0);
            "плюс "
        }
        _ => "",
    };
    let minor = currency.minor_digits as usize;
    let comma = amount.rfind(',');
    let dot = amount.rfind('.');
    match (comma, dot) {
        // Both separators present: the later one is the decimal point.
        (Some(comma), Some(dot)) => {
            let grouping = if comma > dot { '.' } else { ',' };
            amount.retain(|c| c != grouping);
        }
        _ => {
            let separator = if comma.is_some() { ',' } else { '.' };
            let first = amount.find(separator);
            let last = amount.rfind(separator);
            if let (Some(first), Some(last)) = (first, last) {
                if first != last {
                    // Several separators: grouping, possibly with a decimal tail.
                    let trailing = amount.len() - last - 1;
                    if trailing > minor && trailing != 3 {
                        return None;
                    }
                    let keep_last = trailing == minor;
                    let mut normalized = String::with_capacity(amount.len());
                    for (i, ch) in amount.char_indices() {
                        if ch != separator || (i == last && keep_last) {
                            normalized.push(ch);
                        }
                    }
                    amount = normalized;
                } else if (1..=3).contains(&first)
                    && !amount.starts_with('0')
                    && amount.len() - first - 1 == 3
                    && minor != 3
                {
                    // A lone separator with exactly three digits after it groups
                    // thousands rather than opening a fraction.
                    amount.remove(first);
                }
            }
        }
    }

    let pos = amount.find(['.', ',']);
    let main_text = pos.map_or(amount.as_str(), |pos| &amount[..pos]);
    let main = if main_text.is_empty() { 0 } else { try_parse_u64(main_text)? };

    if let Some(pos) = pos {
        // Currencies with no minor unit read the fraction as a proper fraction.
        if minor == 0 {
            let int_part = if main_text.is_empty() { "0" } else { main_text };
            let words = decimal_to_words(int_part, &amount[pos + 1..])?;
            return Some(format!("{sign}{words} {}", currency.main.many));
        }
    }
    let mut sub_units = 0;
    if let Some(pos) = pos {
        let mut frac = amount[pos + 1..].to_owned();
        if frac.len() > minor {
            return None;
        }
        while frac.len() < minor {
            frac.push('0');
        }
        sub_units = try_parse_u64(&frac).unwrap_or(0);
    }
    let mut main_words = split_words(&number_to_words(main));
    if currency.main_feminine {
        feminine_last(&mut main_words);
    }
    let mut out = format!("{sign}{} {}", join(&main_words), plural(main, &currency.main));
    if sub_units > 0 {
        let mut sub_words = split_words(&number_to_words(sub_units));
        if currency.sub_feminine {
            feminine_last(&mut sub_words);
        }
        out.push_str(&format!(" {} {}", join(&sub_words), plural(sub_units, &currency.sub)));
    }
    Some(out)
}

/// True when the match begins in the middle of a longer number.
fn starts_inside_number(prefix: &str) -> bool {
    prefix.chars().next_back().is_some_and(|c| c.is_ascii_digit() || c == '.' || c == ',')
}

/// The patterns that spot an amount for one currency, in matching order.
static CURRENCY_PATTERNS: Lazy<Vec<(usize, Vec<Regex>)>> = Lazy::new(|| {
    let amount = amount_group();
    lexicon::CURRENCIES
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let symbol = escape(entry.symbol);
            let mut patterns = Vec::new();
            match (!entry.word_re.is_empty(), !symbol.is_empty()) {
                (true, true) => patterns.push(compile_i(&format!(
                    r"{amount}\s*({}(?![а-яіїєґ])|{symbol})",
                    entry.word_re
                ))),
                (true, false) => patterns
                    .push(compile_i(&format!(r"{amount}\s*({}(?![а-яіїєґ]))", entry.word_re))),
                (false, true) => patterns.push(compile(&format!(r"{amount}\s*(?:{symbol})"))),
                (false, false) => {}
            }
            if !symbol.is_empty() {
                patterns.push(compile(&format!(r"{symbol}\s*{amount}")));
                if entry.trailing_symbol {
                    patterns.push(compile(&format!(r"(\d+)\s*{symbol}")));
                }
            }
            (index, patterns)
        })
        .collect()
});

static CURRENCY_BY_CODE: Lazy<HashMap<String, usize>> = Lazy::new(|| {
    lexicon::CURRENCIES.iter().enumerate().map(|(i, c)| (lower_text(c.code), i)).collect()
});

/// Reads currency amounts written with a symbol, a word form or an ISO code.
pub(crate) fn normalize_currency(text: &str) -> String {
    static SIGNED_PREFIX: Lazy<Regex> =
        Lazy::new(|| compile_i(&format!(r"([+-])\s*({})\s*(?=\d)", *CURRENCY_TOKEN_ALT)));
    static ACCOUNTING_PREFIX: Lazy<Regex> = Lazy::new(|| {
        compile_i(&format!(r"\(({})\s*(\d+(?:[.,]\d{{1,4}})?)\)", *CURRENCY_TOKEN_ALT))
    });
    static ACCOUNTING: Lazy<Regex> = Lazy::new(|| {
        compile_i(&format!(r"\((\d+(?:[.,]\d{{1,4}})?)\s*({})\)", *CURRENCY_TOKEN_ALT))
    });
    static SUFFIX_CODE: Lazy<Regex> = Lazy::new(|| {
        compile_i(&format!(
            r"{}\s*({})(?![A-Za-zА-Яа-яЄєІіЇїҐґ])",
            amount_group(),
            *CURRENCY_CODE_ALT
        ))
    });
    static PREFIX_CODE: Lazy<Regex> = Lazy::new(|| {
        compile_i(&format!(
            r"({})\s*{}(?![A-Za-zА-Яа-яЄєІіЇїҐґ])",
            *CURRENCY_CODE_ALT,
            amount_group()
        ))
    });

    let text = normalize_regional_currency_aliases(text);
    let text = sub(&text, &SIGNED_PREFIX, |m| format!("{}{}", cap(m, 2), cap(m, 1)));
    let text = sub(&text, &ACCOUNTING_PREFIX, |m| format!("-{} {}", cap(m, 2), cap(m, 1)));
    let mut text = sub(&text, &ACCOUNTING, |m| format!("-{} {}", cap(m, 1), cap(m, 2)));

    for (index, patterns) in CURRENCY_PATTERNS.iter() {
        let currency = &lexicon::CURRENCIES[*index];
        for re in patterns {
            text = sub_ctx(&text, re, |m, prefix| {
                if starts_inside_number(prefix) {
                    return whole(m).to_owned();
                }
                amount_words(cap(m, 1), currency).unwrap_or_else(|| whole(m).to_owned())
            });
        }
    }

    let text = sub_ctx(&text, &SUFFIX_CODE, |m, prefix| {
        if starts_inside_number(prefix) {
            return whole(m).to_owned();
        }
        match CURRENCY_BY_CODE.get(&lower_text(cap(m, 2))) {
            Some(&index) => amount_words(cap(m, 1), &lexicon::CURRENCIES[index])
                .unwrap_or_else(|| whole(m).to_owned()),
            None => whole(m).to_owned(),
        }
    });
    sub(&text, &PREFIX_CODE, |m| match CURRENCY_BY_CODE.get(&lower_text(cap(m, 1))) {
        Some(&index) => amount_words(cap(m, 2), &lexicon::CURRENCIES[index])
            .unwrap_or_else(|| whole(m).to_owned()),
        None => whole(m).to_owned(),
    })
}

fn canonical_code(code: &str) -> String {
    code.to_ascii_uppercase()
}

/// The "many" form of a currency whose code is exactly `code`.
fn currency_many(code: &str) -> Option<&'static str> {
    lexicon::CURRENCIES.iter().find(|c| c.code == code).map(|c| c.main.many)
}

/// How a ticker is spoken: a known reading, a currency name, or spelled out.
fn ticker_words(ticker: &str) -> String {
    let code = canonical_code(ticker);
    if let Some(unit) = FINANCE_UNITS.get(code.as_str()) {
        return unit.forms.many.to_owned();
    }
    if let Some(name) = currency_many(&code) {
        return name.to_owned();
    }
    let parts: Vec<String> = code
        .chars()
        .map(|ch| match ch.to_digit(10) {
            Some(d) => number_to_words(u64::from(d)),
            None => spell_identifier_letters(&ch.to_string()),
        })
        .collect();
    join(&parts)
}

/// Rewrites a grouped amount into a plain one, or `None` when the grouping is
/// inconsistent and the amount cannot be trusted.
fn canonical_amount(raw: &str) -> Option<String> {
    let mut amount: String = raw.chars().filter(|&c| c != ' ' && c != NBSP).collect();
    let comma = amount.rfind(',');
    let dot = amount.rfind('.');
    match (comma, dot) {
        (Some(comma), Some(dot)) => {
            let grouping = if comma > dot { '.' } else { ',' };
            amount.retain(|c| c != grouping);
            if grouping == '.' {
                // The decimal separator was the comma; make it a dot.
                if let Some(at) = amount.rfind(',') {
                    amount.replace_range(at..at + 1, ".");
                }
            }
        }
        (None, None) => {}
        _ => {
            let separator = if comma.is_some() { ',' } else { '.' };
            let first = amount.find(separator).expect("one separator is present");
            let last = amount.rfind(separator).expect("one separator is present");
            let mut grouped;
            if first != last {
                // Every group after the first separator must be three digits.
                grouped = true;
                let mut group_start = first + 1;
                while group_start < amount.len() {
                    let group_end =
                        amount[group_start..].find(separator).map(|offset| group_start + offset);
                    let length = group_end.unwrap_or(amount.len()) - group_start;
                    if length != 3 {
                        grouped = false;
                        break;
                    }
                    match group_end {
                        Some(end) => group_start = end + 1,
                        None => break,
                    }
                }
            } else {
                grouped = amount.len() - first - 1 == 3 && &amount[..first] != "0";
            }
            if grouped {
                amount.retain(|c| c != separator);
            } else if first != last {
                return None;
            } else if separator == ',' {
                amount.replace_range(first..first + 1, ".");
            }
        }
    }
    let decimal = amount.find('.').unwrap_or(amount.len());
    try_parse_u64(&amount[..decimal])?;
    Some(amount)
}

/// Reads amounts paired with cryptocurrency and finance tickers.
///
/// `include_generic` also spells out unknown uppercase tickers, which the
/// pipeline only does on its second pass.
pub(crate) fn normalize_finance(text: &str, include_generic: bool) -> String {
    static TICKER_ALT: Lazy<String> =
        Lazy::new(|| regex_alternation(lexicon::FINANCE_UNITS.iter().map(|u| u.code)));
    static RECOGNIZED_TICKER_ALT: Lazy<String> = Lazy::new(|| {
        regex_alternation(
            lexicon::FINANCE_UNITS
                .iter()
                .map(|u| u.code)
                .chain(lexicon::CURRENCIES.iter().map(|c| c.code)),
        )
    });
    static BITCOIN_PREFIX: Lazy<Regex> =
        Lazy::new(|| compile(&format!(r"₿\s*([+-]?)({RAW_AMOUNT})")));
    static BITCOIN_SUFFIX: Lazy<Regex> =
        Lazy::new(|| compile(&format!(r"([+-]?)({RAW_AMOUNT})\s*₿")));
    static KNOWN_PAIR: Lazy<Regex> = Lazy::new(|| {
        compile_i(&format!(r"\b({})/({})\b", *RECOGNIZED_TICKER_ALT, *RECOGNIZED_TICKER_ALT))
    });
    static PAIR: Lazy<Regex> =
        Lazy::new(|| compile(&format!(r"\b({GENERIC_TICKER})/({GENERIC_TICKER})\b")));

    let text = sub(text, &BITCOIN_PREFIX, |m| format!("{}{} BTC", cap(m, 1), cap(m, 2)));
    let text = sub(&text, &BITCOIN_SUFFIX, |m| format!("{}{} BTC", cap(m, 1), cap(m, 2)));
    let mut text = sub(&text, &KNOWN_PAIR, |m| {
        format!("{} до {}", ticker_words(cap(m, 1)), ticker_words(cap(m, 2)))
    });

    /// Reads one amount, or `None` when the ticker is really a currency or the
    /// amount's grouping does not hold up.
    fn render_amount(raw: &str, ticker: &str, sign: &str) -> Option<String> {
        let code = canonical_code(ticker);
        if currency_many(&code).is_some() {
            return None;
        }
        let amount = canonical_amount(raw)?;
        let spoken_sign = match sign {
            "-" => "мінус ",
            "+" => "плюс ",
            _ => "",
        };
        if let Some(unit) = FINANCE_UNITS.get(code.as_str()) {
            return Some(format!("{spoken_sign}{}", finance_amount_words(&amount, unit)));
        }
        let spoken = ticker_words(&code);
        Some(format!(
            "{spoken_sign}{}",
            finance_amount_words_parts(&amount, &spoken, &spoken, &spoken, &spoken, false)
        ))
    }

    let normalize_amounts = |text: &str,
                             ticker_pattern: &str,
                             ignore_case: bool,
                             prefix_pattern: &str| {
        let boundary = r"[^\wА-Яа-яЄєІіЇїҐґ]";
        let make = |p: &str| if ignore_case { compile_i(p) } else { compile(p) };
        let suffix =
            make(&format!(r"(^|{boundary})([+-]?)((?:{RAW_AMOUNT}))\s*({ticker_pattern})\b"));
        let text = sub(text, &suffix, |m| match render_amount(cap(m, 3), cap(m, 4), cap(m, 2)) {
            Some(words) => format!("{}{words}", cap(m, 1)),
            None => whole(m).to_owned(),
        });
        let prefix = make(&format!(r"\b({prefix_pattern})\s*([+-]?)((?:{RAW_AMOUNT}))(?![\w.,])"));
        sub(&text, &prefix, |m| {
            render_amount(cap(m, 3), cap(m, 1), cap(m, 2)).unwrap_or_else(|| whole(m).to_owned())
        })
    };

    text = normalize_amounts(&text, &TICKER_ALT, true, &TICKER_ALT);
    if !include_generic {
        return text;
    }

    let is_recognized = |ticker: &str| {
        let code = canonical_code(ticker);
        FINANCE_UNITS.contains_key(code.as_str()) || currency_many(&code).is_some()
    };
    let text = sub(&text, &PAIR, |m| {
        // An arbitrary A/B token is more often a protocol or standard (TCP/IP,
        // ISO/IEC) than a market pair. Use "slash" unless one side anchors the
        // expression in the finance lexicon.
        let separator =
            if is_recognized(cap(m, 1)) || is_recognized(cap(m, 2)) { "до" } else { "слеш" };
        format!("{} {separator} {}", ticker_words(cap(m, 1)), ticker_words(cap(m, 2)))
    });
    // Unknown suffix tickers ("5 NEWCOIN") are unambiguous enough to spell.
    // Unknown prefix tokens are deliberately left alone: "ISO 3166",
    // "IEEE 802.3" and "ALGOL 58" have the same surface form as "XYZ 5".
    normalize_amounts(&text, GENERIC_TICKER, false, "(?!)")
}
