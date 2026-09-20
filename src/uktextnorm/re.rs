//! Regex helpers.
//!
//! The reference implementation uses ECMAScript `std::regex`, whose `\d`, `\w`,
//! `\s` and `\b` are ASCII-only. Rust's regex syntax makes them Unicode-aware,
//! which changes where a boundary falls in Cyrillic text. [`translate`] rewrites
//! those escapes into their ASCII equivalents so the patterns below can be read
//! side by side with the originals.

use fancy_regex::{Captures, Regex};

/// The characters ECMAScript's `\w` covers.
const WORD: &str = "0-9A-Za-z_";
/// An ASCII word boundary, spelled with lookarounds because Rust's `\b` is
/// Unicode-aware and inline `(?-u:)` is not supported by the engine.
const BOUNDARY: &str =
    concat!("(?:(?<=[0-9A-Za-z_])(?![0-9A-Za-z_])", "|(?<![0-9A-Za-z_])(?=[0-9A-Za-z_]))");
const NOT_BOUNDARY: &str =
    concat!("(?:(?<=[0-9A-Za-z_])(?=[0-9A-Za-z_])", "|(?<![0-9A-Za-z_])(?![0-9A-Za-z_]))");
const SPACE: &str = r" \t\n\r\x0B\x0C";

/// Rewrites ECMAScript's ASCII-only escapes into explicit Rust equivalents.
pub(crate) fn translate(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len() + 32);
    let mut chars = pattern.chars().peekable();
    let mut in_class = false;
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            match ch {
                '[' if !in_class => in_class = true,
                ']' if in_class => in_class = false,
                _ => {}
            }
            out.push(ch);
            continue;
        }
        let Some(escaped) = chars.next() else {
            out.push('\\');
            break;
        };
        // Inside a character class only the class body is emitted; outside, a
        // whole class is.
        let (bare, wrapped, negated) = match escaped {
            'd' => ("0-9", "[0-9]", "[^0-9]"),
            'w' => (WORD, "[0-9A-Za-z_]", "[^0-9A-Za-z_]"),
            's' => (SPACE, "[ \\t\\n\\r\\x0B\\x0C]", "[^ \\t\\n\\r\\x0B\\x0C]"),
            'b' if !in_class => ("", BOUNDARY, ""),
            'B' if !in_class => ("", NOT_BOUNDARY, ""),
            'b' => ("", "\\x08", ""),
            'D' | 'W' | 'S' => {
                let negated = match escaped {
                    'D' => "[^0-9]",
                    'W' => "[^0-9A-Za-z_]",
                    _ => "[^ \\t\\n\\r\\x0B\\x0C]",
                };
                out.push_str(negated);
                continue;
            }
            _ => {
                out.push('\\');
                out.push(escaped);
                continue;
            }
        };
        let _ = negated;
        out.push_str(if in_class { bare } else { wrapped });
    }
    out
}

/// Compiles a pattern written in the reference implementation's dialect.
///
/// # Panics
///
/// Panics when the pattern is invalid. All patterns are crate-internal
/// constants, so a failure here is a bug rather than bad input.
pub(crate) fn compile(pattern: &str) -> Regex {
    let translated = translate(pattern);
    Regex::new(&translated)
        .unwrap_or_else(|e| panic!("invalid pattern {pattern:?} -> {translated:?}: {e}"))
}

/// Compiles a case-insensitive pattern, folding ASCII only.
///
/// `std::regex`'s `icase` uses the C locale and so folds nothing outside
/// ASCII; Rust's `(?i)` is Unicode-aware and would also match `СІЧНЯ` where
/// the reference implementation matches only `січня` and `Січня`. Wrapping
/// every non-ASCII literal in `(?-i:…)` restores the reference behaviour.
pub(crate) fn compile_i(pattern: &str) -> Regex {
    compile(&format!("(?i){}", ascii_case_only(pattern)))
}

/// Protects non-ASCII literals from case folding.
fn ascii_case_only(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    let mut chars = pattern.char_indices().peekable();
    while let Some((i, ch)) = chars.next() {
        match ch {
            '\\' => {
                out.push(ch);
                if let Some((_, escaped)) = chars.next() {
                    out.push(escaped);
                }
            }
            '[' => {
                // Find this class's closing bracket, allowing a leading `^`,
                // a literal `]` in first position, and escapes.
                let rest = &pattern[i..];
                let mut body = rest.char_indices().skip(1).peekable();
                if matches!(body.peek(), Some((_, '^'))) {
                    body.next();
                }
                if matches!(body.peek(), Some((_, ']'))) {
                    body.next();
                }
                let mut end = None;
                while let Some((offset, c)) = body.next() {
                    match c {
                        '\\' => {
                            body.next();
                        }
                        ']' => {
                            end = Some(i + offset + 1);
                            break;
                        }
                        _ => {}
                    }
                }
                let Some(end) = end else {
                    out.push(ch);
                    continue;
                };
                let class = &pattern[i..end];
                if class.is_ascii() {
                    out.push_str(class);
                } else {
                    out.push_str("(?-i:");
                    out.push_str(class);
                    out.push(')');
                }
                while chars.peek().is_some_and(|&(j, _)| j < end) {
                    chars.next();
                }
            }
            // Each character is wrapped on its own so a following quantifier
            // still applies to that one character.
            c if !c.is_ascii() => {
                out.push_str("(?-i:");
                out.push(c);
                out.push(')');
            }
            c => out.push(c),
        }
    }
    out
}

/// Replaces every match of `re` in `text` with the result of `f`.
///
/// The replacement is inserted literally: `$1` in the returned string is not
/// expanded, matching the reference implementation's substitution helper.
pub(crate) fn sub<F>(text: &str, re: &Regex, mut f: F) -> String
where
    F: FnMut(&Captures<'_, str>) -> String,
{
    sub_ctx(text, re, |caps, _| f(caps))
}

/// The text around a match, as `std::sregex_iterator` exposes it.
pub(crate) struct MatchContext<'t> {
    /// The text between the previous match and this one.
    pub prefix: &'t str,
    /// The text from the end of this match to the end of the input.
    pub suffix: &'t str,
}

/// Like [`sub`], but also hands `f` the text around the match.
pub(crate) fn sub_ctx<F>(text: &str, re: &Regex, mut f: F) -> String
where
    F: FnMut(&Captures<'_, str>, &str) -> String,
{
    sub_around(text, re, |caps, ctx| f(caps, ctx.prefix))
}

/// Like [`sub_ctx`], but hands `f` both the prefix and the suffix.
pub(crate) fn sub_around<F>(text: &str, re: &Regex, mut f: F) -> String
where
    F: FnMut(&Captures<'_, str>, &MatchContext<'_>) -> String,
{
    let mut out: Option<String> = None;
    let mut last = 0;
    let mut pos = 0;
    while pos <= text.len() {
        let Ok(Some(caps)) = re.captures_from_pos(text, pos) else { break };
        let m = caps.get(0).expect("group 0 always participates");
        let out = out.get_or_insert_with(|| String::with_capacity(text.len()));
        out.push_str(&text[last..m.start()]);
        let ctx = MatchContext { prefix: &text[last..m.start()], suffix: &text[m.end()..] };
        let replacement = f(&caps, &ctx);
        out.push_str(&replacement);
        last = m.end();
        // An empty match would otherwise spin in place.
        pos = if m.end() == m.start() { next_boundary(text, m.end()) } else { m.end() };
    }
    match out {
        Some(mut out) => {
            out.push_str(&text[last..]);
            out
        }
        // Nothing matched, so hand back the input untouched.
        None => text.to_owned(),
    }
}

/// Visits every match of `re` in `text`, left to right.
pub(crate) fn each<F>(text: &str, re: &Regex, mut f: F)
where
    F: FnMut(&Captures<'_, str>),
{
    let mut pos = 0;
    while pos <= text.len() {
        let Ok(Some(caps)) = re.captures_from_pos(text, pos) else { break };
        let m = caps.get(0).expect("group 0 always participates");
        let end = m.end();
        f(&caps);
        pos = if end == m.start() { next_boundary(text, end) } else { end };
    }
}

/// The next character boundary at or after `index + 1`.
fn next_boundary(text: &str, index: usize) -> usize {
    let mut next = index + 1;
    while next < text.len() && !text.is_char_boundary(next) {
        next += 1;
    }
    next
}

/// The text of capture group `index`, or `""` when the group did not take part.
pub(crate) fn cap<'t>(caps: &Captures<'t, str>, index: usize) -> &'t str {
    caps.get(index).map_or("", |m| m.as_str())
}

/// Whether capture group `index` took part in the match.
pub(crate) fn matched(caps: &Captures<'_, str>, index: usize) -> bool {
    caps.get(index).is_some()
}

/// The byte offset where capture group `index` starts.
pub(crate) fn cap_start(caps: &Captures<'_, str>, index: usize) -> usize {
    caps.get(index).map_or(0, |m| m.start())
}

/// The whole match.
pub(crate) fn whole<'t>(caps: &Captures<'t, str>) -> &'t str {
    cap(caps, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_ascii_escapes() {
        assert_eq!(translate(r"\d+"), "[0-9]+");
        assert_eq!(translate(r"[\d.,]"), "[0-9.,]");
        assert_eq!(translate(r"\w"), "[0-9A-Za-z_]");
        assert!(translate(r"\bfoo").starts_with("(?:(?<="));
    }

    #[test]
    fn case_insensitivity_is_ascii_only() {
        let re = compile_i("січня|Січня");
        assert!(re.is_match("січня").unwrap());
        assert!(re.is_match("Січня").unwrap());
        // The reference implementation does not fold Cyrillic.
        assert!(!re.is_match("СІЧНЯ").unwrap());
        assert!(compile_i("abc").is_match("ABC").unwrap());
    }

    #[test]
    fn word_boundary_is_ascii_only() {
        // Rust's Unicode `\b` would not see a boundary between `к` and `2`.
        let re = compile(r"\b\d{4}\b");
        assert_eq!(re.find("рік2024ось").unwrap().map(|m| m.as_str()), Some("2024"));
        assert!(re.find("x2024y").unwrap().is_none());
    }
}
