//! Passes that work on the shape of the text: Unicode cleanup, typography,
//! web addresses, postal addresses, digit grouping, section references,
//! symbols and phone numbers.

use fancy_regex::Regex;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::sync::LazyLock;

use crate::uktextnorm::numbers::number_to_words_digit_by_digit;
use crate::uktextnorm::re::{cap, compile, compile_i, sub, sub_ctx, whole};
use crate::uktextnorm::readers::normalize_phone_number;
use crate::uktextnorm::text::{
    is_latin, is_uk, is_upper_uk, is_word_joiner, lower_text, trim_spaces, uncertain_word_spans,
};
use crate::uktextnorm::validation::valid_roman;
use crate::uktextnorm::{PhoneStyle, QuoteStyle};

fn is_quote(cp: char) -> bool {
    matches!(cp, '«' | '»' | '„' | '“' | '”' | '‟' | '‹' | '›' | '"')
}

fn is_opening_quote(cp: char) -> bool {
    matches!(cp, '«' | '„' | '‟' | '‹')
}

/// Characters sometimes typed in place of an apostrophe.
fn is_apostrophe_variant(cp: char) -> bool {
    matches!(cp, 'ʼ' | '´' | '′' | '‛' | '‘')
}

fn is_uk_letter(cp: char) -> bool {
    is_uk(cp) && !is_word_joiner(cp)
}

fn is_word_alphanumeric(cp: char) -> bool {
    is_uk_letter(cp) || is_latin(cp) || cp.is_ascii_digit()
}

/// Composes Ukrainian letters written with combining marks, canonicalizes
/// apostrophes, dashes and signs, and applies the quote style.
pub(crate) fn normalize_unicode(text: &str, quote_style: QuoteStyle) -> String {
    let cps: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut idx = 0;
    while idx < cps.len() {
        let cp = cps[idx];
        let next = cps.get(idx + 1).copied().unwrap_or('\0');
        let prev = if idx > 0 { cps[idx - 1] } else { '\0' };

        // Targeted NFC for Ukrainian: compose и/і with a combining breve or diaeresis.
        if next == '\u{306}' && (cp == 'и' || cp == 'И') {
            out.push(if cp == 'и' { 'й' } else { 'Й' });
            idx += 2;
            continue;
        }
        if next == '\u{308}' && (cp == 'і' || cp == 'І') {
            out.push(if cp == 'і' { 'ї' } else { 'Ї' });
            idx += 2;
            continue;
        }

        let prime = cp == '′';
        let inside_word = if prime {
            (is_uk_letter(prev) || is_latin(prev)) && (is_uk_letter(next) || is_latin(next))
        } else {
            is_word_alphanumeric(prev) && is_word_alphanumeric(next)
        };
        if is_apostrophe_variant(cp) && inside_word {
            out.push('\'');
            idx += 1;
            continue;
        }

        // The tightly joined technical notation 10−n is a power of ten, so this
        // one minus sign survives until the scientific pass.
        let power_of_ten = cp == '−'
            && idx >= 2
            && cps[idx - 2] == '1'
            && prev == '0'
            && (idx == 2 || !cps[idx - 3].is_ascii_digit())
            && next.is_ascii_digit();
        if power_of_ten {
            out.push(cp);
            idx += 1;
            continue;
        }
        match cp {
            '−' | '－' => {
                out.push('-');
                idx += 1;
                continue;
            }
            '＋' => {
                out.push('+');
                idx += 1;
                continue;
            }
            '‐' | '‑' | '‒' => {
                out.push('–');
                idx += 1;
                continue;
            }
            _ => {}
        }

        if is_quote(cp) && quote_style != QuoteStyle::Keep {
            match quote_style {
                QuoteStyle::Straight => out.push('"'),
                QuoteStyle::Guillemets => {
                    out.push(if is_opening_quote(cp) || cp == '“' { '«' } else { '»' });
                }
                QuoteStyle::Strip => {
                    // A quote between two word characters leaves a space behind.
                    if is_word_alphanumeric(prev)
                        && is_word_alphanumeric(next)
                        && !out.ends_with(' ')
                    {
                        out.push(' ');
                    }
                }
                QuoteStyle::Keep => {}
            }
            idx += 1;
            continue;
        }
        out.push(cp);
        idx += 1;
    }
    out
}

/// Latin letters that look identical to a Cyrillic one.
#[rustfmt::skip]
static LATIN_TO_CYRILLIC: LazyLock<HashMap<char, char>> = LazyLock::new(|| {
    [
        ('a', 'а'), ('e', 'е'), ('i', 'і'), ('o', 'о'), ('p', 'р'), ('c', 'с'), ('x', 'х'),
        ('y', 'у'), ('A', 'А'), ('B', 'В'), ('C', 'С'), ('E', 'Е'), ('H', 'Н'), ('I', 'І'),
        ('K', 'К'), ('M', 'М'), ('O', 'О'), ('P', 'Р'), ('T', 'Т'), ('X', 'Х'),
    ]
    .into_iter()
    .collect()
});

static CYRILLIC_TO_LATIN: LazyLock<HashMap<char, char>> =
    LazyLock::new(|| LATIN_TO_CYRILLIC.iter().map(|(&l, &c)| (c, l)).collect());

/// Repairs words that mix Latin and Cyrillic letters that look the same,
/// converting the minority script into the majority one.
pub(crate) fn normalize_homoglyphs(text: &str) -> String {
    let words = uncertain_word_spans(text);
    if words.is_empty() {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for word in &words {
        let slice = &text[word.start..word.stop];
        let latin_count = slice.chars().filter(|&c| is_latin(c)).count();
        let cyr_count = slice.chars().filter(|&c| is_uk_letter(c)).count();
        if latin_count == 0 || cyr_count == 0 {
            continue;
        }
        let to_cyrillic = cyr_count >= latin_count;
        let map = if to_cyrillic { &*LATIN_TO_CYRILLIC } else { &*CYRILLIC_TO_LATIN };
        let mut repaired = String::with_capacity(slice.len());
        let mut repairable = true;
        for cp in slice.chars() {
            let minority = if to_cyrillic { is_latin(cp) } else { is_uk_letter(cp) };
            if !minority {
                repaired.push(cp);
            } else if let Some(&replacement) = map.get(&cp) {
                repaired.push(replacement);
            } else {
                // A letter with no lookalike means this is genuinely mixed text.
                repairable = false;
                break;
            }
        }
        if !repairable {
            continue;
        }
        out.push_str(&text[last..word.start]);
        out.push_str(&repaired);
        last = word.stop;
    }
    out.push_str(&text[last..]);
    out
}

/// Removes runs of exactly two `marker` characters, which mark Markdown emphasis.
fn strip_exact_pair(value: &str, marker: char) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(ch) = rest.chars().next() {
        if ch != marker {
            out.push(ch);
            rest = &rest[ch.len_utf8()..];
            continue;
        }
        let run = rest.chars().take_while(|&c| c == marker).count();
        if run != 2 {
            out.push_str(&rest[..run]);
        }
        rest = &rest[run..];
    }
    out
}

/// Normalizes spaces, Markdown emphasis, apostrophes, spacing around
/// punctuation and slashed units.
pub(crate) fn normalize_typography(text: &str) -> String {
    static SPACE_BEFORE_PUNCTUATION: LazyLock<Regex> =
        LazyLock::new(|| compile(r"[ \t]+(\.(?=\d|[.,;:!?]|$)|[,;:!?])"));
    static SPACED_UNIT_SLASH: LazyLock<Regex> = LazyLock::new(|| {
        compile(concat!(
            r"(км|м|см³|см3|кбіт|Кбіт|мбіт|Мбіт|гбіт|Гбіт)",
            r"[ \t]*/[ \t]*(год|с(?:²|2)?|c(?:²|2)?)(?![A-Za-z])"
        ))
    });
    static SPACED_ASCII_SLASH: LazyLock<Regex> =
        LazyLock::new(|| compile(r"([A-Za-z])[ \t]*/[ \t]*([A-Za-z])"));
    static SPACED_CYRILLIC_DIMENSION: LazyLock<Regex> =
        LazyLock::new(|| compile(r"(\d)\s+(?:х|Х)\s+(\d)"));

    let mut text = text.to_owned();
    for space in ['\u{a0}', '\u{2009}', '\u{202f}', '\u{2060}'] {
        text = text.replace(space, " ");
    }
    text = strip_exact_pair(&text, '*');
    text = strip_exact_pair(&text, '_');
    text = text.replace('`', "").replace('’', "'");
    let text = sub(&text, &SPACE_BEFORE_PUNCTUATION, |m| cap(m, 1).to_owned());
    let text = sub(&text, &SPACED_UNIT_SLASH, |m| {
        let denominator = cap(m, 2);
        // A Latin `c` standing in for Cyrillic `с` in a mixed-script unit.
        let denominator = match denominator.strip_prefix('c') {
            Some(rest) => format!("с{rest}"),
            None => denominator.to_owned(),
        };
        format!("{}/{denominator}", cap(m, 1))
    });
    let text = sub(&text, &SPACED_ASCII_SLASH, |m| format!("{}/{}", cap(m, 1), cap(m, 2)));
    sub(&text, &SPACED_CYRILLIC_DIMENSION, |m| format!("{} × {}", cap(m, 1), cap(m, 2)))
}

/// Spells a web address out symbol by symbol.
fn spell_web(value: &str) -> String {
    static UKRAINIAN_DOMAIN_LABEL: LazyLock<Regex> =
        LazyLock::new(|| compile_i(r"\.(ua|укр)(?=$|[/?#])"));
    let mut s = value.trim_end_matches(['.', ',', '!', '?']).to_owned();
    s = sub(&s, &UKRAINIAN_DOMAIN_LABEL, |m| {
        if lower_text(cap(m, 1)) == "ua" {
            " крапка ю ей ".into()
        } else {
            " крапка укр ".into()
        }
    });
    for (symbol, word) in [
        ("@", " равлик "),
        (".", " крапка "),
        ("/", " слеш "),
        (":", " двокрапка "),
        ("-", " дефіс "),
        ("_", " підкреслення "),
        ("?", " знак питання "),
        ("=", " дорівнює "),
        ("&", " амперсанд "),
        ("#", " решітка "),
        ("+", " плюс "),
    ] {
        s = s.replace(symbol, word);
    }
    trim_spaces(&s)
}

/// Reads DOIs, emails, URLs, hashtags and handles aloud.
pub(crate) fn normalize_web(text: &str) -> String {
    static DOI: LazyLock<Regex> =
        LazyLock::new(|| compile_i(r"\bdoi\s*:\s*(10\.\d{4,9}/[-._;()/:A-Za-z0-9]*[A-Za-z0-9])"));
    static EMAIL: LazyLock<Regex> = LazyLock::new(|| {
        compile(concat!(
            r"(^|[^A-Za-zА-Яа-яЄєІіЇїҐґ0-9._%+\-])",
            r"([A-Za-zА-Яа-яЄєІіЇїҐґ0-9._%+\-]+@[A-Za-zА-Яа-яЄєІіЇїҐґ0-9\-]+",
            r"(?:\.[A-Za-zА-Яа-яЄєІіЇїҐґ0-9\-]+)+)"
        ))
    });
    static URL: LazyLock<Regex> = LazyLock::new(|| {
        compile_i(concat!(
            r"\b(?:(?:https?|ftp)://|www\.)\S+",
            r"|\b(?:[A-Za-zА-Яа-яЄєІіЇїҐґ0-9-]+\.)+[A-Za-zА-Яа-яЄєІіЇїҐґ]{2,63}(?:[/?#]\S*)?"
        ))
    });
    static STANDALONE_DOMAIN: LazyLock<Regex> = LazyLock::new(|| {
        compile_i(r"(^|[^A-Za-zА-Яа-яЄєІіЇїҐґ0-9])\.(ua|укр)(?![A-Za-zА-Яа-яЄєІіЇїҐґ0-9])")
    });
    static HASHTAG: LazyLock<Regex> = LazyLock::new(|| compile(r"#([A-Za-zА-Яа-яЄєІіЇїҐґ0-9_]+)"));
    static HANDLE: LazyLock<Regex> = LazyLock::new(|| {
        compile(r"(^|[^A-Za-zА-Яа-яЄєІіЇїҐґ0-9._%+-])@([A-Za-z][A-Za-z0-9_]{1,30})")
    });

    let text = sub(text, &DOI, |m| format!("ді оу ай {}", spell_web(cap(m, 1))));
    let text = sub(&text, &EMAIL, |m| format!("{}{}", cap(m, 1), spell_web(cap(m, 2))));
    let text = sub(&text, &URL, |m| spell_web(whole(m)));
    let text = sub(&text, &STANDALONE_DOMAIN, |m| {
        let label = if lower_text(cap(m, 2)) == "ua" {
            "крапка ю ей"
        } else {
            "крапка укр"
        };
        format!("{}{label}", cap(m, 1))
    });
    let text = sub(&text, &HASHTAG, |m| format!("хештег {}", cap(m, 1)));
    sub(&text, &HANDLE, |m| format!("{}акаунт {}", cap(m, 1), cap(m, 2)))
}

/// Expands the abbreviations used in postal addresses, choosing the case that
/// the surrounding preposition calls for.
pub(crate) fn normalize_addresses(text: &str) -> String {
    #[rustfmt::skip]
    static WORDS: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
        [
            ("м", "місто"), ("с", "село"), ("смт", "селище міського типу"), ("вул", "вулиця"),
            ("просп", "проспект"), ("пр", "проспект"), ("пров", "провулок"), ("пл", "площа"),
            ("бул", "бульвар"), ("наб", "набережна"), ("буд", "будинок"), ("б", "будинок"),
            ("кв", "квартира"), ("оф", "офіс"), ("корп", "корпус"), ("під", "під'їзд"),
            ("пов", "поверх"), ("обл", "область"), ("р-н", "район"),
        ]
        .into_iter()
        .collect()
    });
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        compile_i(concat!(
            r"((?:смт|просп|пров|корп|буд|вул|наб|бул|оф|кв|обл|під|пов|р-н|пр|пл|м|с|б(?!\.п)))",
            r"\.(?=\s*[A-Za-zА-Яа-яЄєІіЇїҐґ0-9])"
        ))
    });
    static LOCATIVE_PREPOSITION: LazyLock<Regex> =
        LazyLock::new(|| compile(r"(^|[\s(])(?:у|в)\s+$"));
    static GENITIVE_PREPOSITION: LazyLock<Regex> =
        LazyLock::new(|| compile(r"(^|[\s(])(?:від|до|з|із|зі|для)\s+$"));

    sub(text, &RE, |m| {
        let key = lower_text(cap(m, 1));
        let group = m.get(0).expect("group 0 participates");
        let (start, end) = (group.start(), group.end());

        // An abbreviation must start a token.
        if let Some(previous) = text[..start].chars().next_back() {
            if is_uk_letter(previous)
                || is_latin(previous)
                || previous.is_ascii_digit()
                || previous == '/'
            {
                return whole(m).to_owned();
            }
        }
        // These three only introduce a proper name or a number.
        if matches!(key.as_str(), "м" | "с" | "корп") {
            let following = text[end..].trim_start();
            let cp = following.chars().next().unwrap_or('\0');
            if !is_upper_uk(cp) && !cp.is_ascii_uppercase() && !cp.is_ascii_digit() {
                return whole(m).to_owned();
            }
        }
        if key == "м" {
            let preceding = lower_text(&text[..start]);
            if LOCATIVE_PREPOSITION.is_match(&preceding).unwrap_or(false) {
                return "місті".to_owned();
            }
            if GENITIVE_PREPOSITION.is_match(&preceding).unwrap_or(false) {
                return "міста".to_owned();
            }
            // "м. Києва" is an unambiguous genitive even with no preposition.
            let following = text[end..].trim_start();
            if let Some(rest) = following.strip_prefix("Києва") {
                if !rest.chars().next().is_some_and(is_uk_letter) {
                    return "міста".to_owned();
                }
            }
        }
        // Here "с." is the speed-of-light variable ending a sentence, not a
        // village abbreviation introducing the next one.
        if key == "с" && lower_text(&text[..start]).ends_with("вакуумі ") {
            return whole(m).to_owned();
        }
        WORDS.get(key.as_str()).map_or_else(|| whole(m).to_owned(), |w| (*w).to_owned())
    })
}

/// Joins digits that were grouped with spaces, commas or dots.
pub(crate) fn normalize_number_groups(text: &str, parse_thousand_separators: bool) -> String {
    static LEADING_DECIMAL: LazyLock<Regex> =
        LazyLock::new(|| compile(r"(^|[\s(\[{=:;])([+\-]?)([.,])(\d+)(?![\d.,])"));
    static SPACE_GROUPED: LazyLock<Regex> = LazyLock::new(|| compile(r"\b\d{1,3}(?: \d{3})+\b"));
    static COMMA_GROUPED: LazyLock<Regex> =
        LazyLock::new(|| compile(r"(^|[^\d.,])(\d{1,3}(?:,\d{3}){2,})(?!\d)"));
    static DOT_GROUPED: LazyLock<Regex> =
        LazyLock::new(|| compile(r"(^|[^\d.,])(\d{1,3}(?:\.\d{3}){2})(?!\.?\d)"));

    let text = sub(text, &LEADING_DECIMAL, |m| {
        format!("{}{}0{}{}", cap(m, 1), cap(m, 2), cap(m, 3), cap(m, 4))
    });
    let text = sub(&text, &SPACE_GROUPED, |m| whole(m).replace(' ', ""));
    if !parse_thousand_separators {
        return text;
    }
    // 1,234,567 — at least two comma groups of exactly three digits.
    let text =
        sub(&text, &COMMA_GROUPED, |m| format!("{}{}", cap(m, 1), cap(m, 2).replace(',', "")));
    // 1.234.567 — exactly two dot groups, so IPv4 addresses stay intact.
    sub(&text, &DOT_GROUPED, |m| format!("{}{}", cap(m, 1), cap(m, 2).replace('.', "")))
}

/// Expands `ст.`, `п.`, `ч.` and friends before a number or Roman numeral.
pub(crate) fn normalize_sections(text: &str) -> String {
    #[rustfmt::skip]
    static SECTION: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
        [
            ("ст", "стаття"), ("ч", "частина"), ("пп", "підпункт"), ("п", "пункт"),
            ("абз", "абзац"), ("розд", "розділ"), ("гл", "глава"), ("табл", "таблиця"),
            ("рис", "рисунок"),
        ]
        .into_iter()
        .collect()
    });
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        compile_i(
            r"(^|[^А-Яа-яЄєІіЇїҐґA-Za-z])(ст|ч|пп|п|абз|розд|гл|табл|рис)\.\s*(?=\d|[MDCLXVI])",
        )
    });
    static TRAILING_ROMAN: LazyLock<Regex> = LazyLock::new(|| compile(r"([MDCLXVI]{1,6})\s*$"));

    sub_ctx(text, &RE, |m, prefix| {
        let key = lower_text(cap(m, 2));
        // "XX ст." is a century, not an article reference.
        if key == "ст" {
            if let Ok(Some(previous)) = TRAILING_ROMAN.captures(prefix) {
                if valid_roman(previous.get(1).map_or("", |g| g.as_str())) {
                    return whole(m).to_owned();
                }
            }
        }
        match SECTION.get(key.as_str()) {
            Some(word) => format!("{}{word} ", cap(m, 1)),
            None => whole(m).to_owned(),
        }
    })
}

/// Replaces mathematical and typographic symbols with their spoken names.
pub(crate) fn normalize_symbols(text: &str) -> String {
    // Order matters: longer symbols are replaced before their prefixes.
    #[rustfmt::skip]
    const SYMBOLS: [(&str, &str); 37] = [
        ("°C", "градусів Цельсія"), ("°С", "градусів Цельсія"), ("°F", "градусів Фаренгейта"),
        ("℃", "градусів Цельсія"), ("℉", "градусів Фаренгейта"), ("°Ra", "градусів Ранкіна"),
        ("°Ré", "градусів Реомюра"), ("°Re", "градусів Реомюра"), ("°De", "градусів Деліля"),
        ("°Rø", "градусів Ремера"), ("°Rō", "градусів Ремера"), ("°R", "градусів Ранкіна"),
        ("°K", "кельвінів"), ("°К", "кельвінів"), ("\u{212a}", "кельвінів"), ("±", "плюс мінус"),
        ("≈", "приблизно дорівнює"), ("≠", "не дорівнює"), ("≤", "менше або дорівнює"),
        ("≥", "більше або дорівнює"), ("×", "помножити на"), ("÷", "поділити на"),
        ("=", "дорівнює"), ("<", "менше"), (">", "більше"), ("‰", "проміле"),
        ("§", "параграф"), ("₿", "біткоїн"), ("•", " "), ("·", " "), ("~", "тильда"),
        ("&", "і"), ("#", "решітка"), ("_", "нижнє підкреслення"), ("²", "у квадраті"),
        ("³", "у кубі"), ("№", "номер"),
    ];
    let mut text = text.to_owned();
    for (symbol, word) in SYMBOLS {
        if text.contains(symbol) {
            text = text.replace(symbol, &format!(" {word} "));
        }
    }
    trim_spaces(&text)
}

/// Reads Ukrainian and international phone numbers, including extensions.
pub(crate) fn normalize_text_with_phone_numbers(text: &str, style: PhoneStyle) -> String {
    static UKRAINIAN: LazyLock<Regex> = LazyLock::new(|| {
        compile(concat!(
            r"(^|[^\d.,])((?:\+?380|00380|0)\s*\(?\d{2}\)?[\-\s]?\d{3}[\-\s]?\d{2}[\-\s]?\d{2})",
            r"(?:\s*(?:доб\.?|дод\.?|ext\.?|x)\s*(\d{1,6}))?(?!\d)"
        ))
    });
    static INTERNATIONAL: LazyLock<Regex> = LazyLock::new(|| {
        compile_i(concat!(
            r"(^|[^\d.,])((?:\+|00)\d{1,3}(?:[\s().-]*\d{1,4}){2,})",
            r"(?:\s*(?:доб\.?|дод\.?|ext\.?|x)\s*(\d{1,6}))?(?!\d)"
        ))
    });
    let read = |m: &fancy_regex::Captures<'_, str>| {
        let mut out = format!("{}{}", cap(m, 1), normalize_phone_number(cap(m, 2), style));
        let extension = cap(m, 3);
        if !extension.is_empty() {
            let _ = write!(out, " додатковий {}", number_to_words_digit_by_digit(extension));
        }
        out
    };
    let text = sub(text, &UKRAINIAN, read);
    sub(&text, &INTERNATIONAL, read)
}
