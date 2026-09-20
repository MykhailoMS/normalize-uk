//! Tokenization.

use super::abbrev;
use super::chars::{
    codepoints, is_alpha, is_digit, is_inner_uk_apostrophe, is_latin, is_space, is_uk,
    is_word_mark, lower_ascii_ukrainian, smile_at, Cp,
};
use super::{push_substring, Substring};

const TOKEN_PUNCT: &str = "\\/!#$%&*+,.:;<=>?@^_`|~№…‑–—−-«“‘»”’\"„'()[]{}";

/// The coarse kind of an atom, which drives the joining rules.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AtomType {
    Uk,
    Lat,
    Int,
    Punct,
    Other,
}

/// An indivisible run of characters; tokens are built by joining adjacent atoms.
#[derive(Clone, Copy, Debug)]
struct Atom<'a> {
    start: usize,
    stop: usize,
    kind: AtomType,
    text: &'a str,
}

fn is_token_punct(cp: char) -> bool {
    TOKEN_PUNCT.contains(cp)
}

fn is_web_stop(cp: char) -> bool {
    is_space(cp) || "<>\"«»“”()[]{}".contains(cp)
}

fn is_trailing_url_punct(cp: char) -> bool {
    matches!(cp, '.' | ',' | ';' | ':' | '!' | '?')
}

/// Matches `№`, optionally hyphenated, followed by an identifier body.
fn legal_number_atom_stop(cps: &[Cp], index: usize) -> Option<usize> {
    if cps[index].value != '№' {
        return None;
    }
    let mut i = index + 1;
    if cps.get(i).is_some_and(|c| c.value == '-') {
        i += 1;
    }
    let body_begin = i;
    while i < cps.len()
        && (is_alpha(cps[i].value) || is_digit(cps[i].value) || matches!(cps[i].value, '/' | '-'))
    {
        i += 1;
    }
    (i > body_begin).then_some(i)
}

/// Matches a URL, an `@handle`, a `#hashtag` or an email address.
fn web_atom_stop(text: &str, cps: &[Cp], index: usize) -> Option<usize> {
    let rest = &text[cps[index].start..];
    if rest.starts_with("http://") || rest.starts_with("https://") {
        let mut i = index;
        while i < cps.len() && !is_web_stop(cps[i].value) {
            i += 1;
        }
        while i > index && is_trailing_url_punct(cps[i - 1].value) {
            i -= 1;
        }
        return (i > index).then_some(i);
    }

    if matches!(cps[index].value, '@' | '#') {
        let mut i = index + 1;
        while i < cps.len()
            && (is_alpha(cps[i].value) || is_digit(cps[i].value) || is_word_mark(cps[i].value))
        {
            i += 1;
        }
        return (i > index + 1).then_some(i);
    }

    if !is_alpha(cps[index].value) && !is_digit(cps[index].value) {
        return None;
    }
    let mut i = index;
    let mut saw_at = false;
    let mut saw_dot_after_at = false;
    while i < cps.len() {
        let cp = cps[i].value;
        if is_alpha(cp) || is_digit(cp) || matches!(cp, '_' | '-' | '.') {
            saw_dot_after_at |= saw_at && cp == '.';
            i += 1;
        } else if cp == '@' && !saw_at && i > index {
            saw_at = true;
            i += 1;
        } else {
            break;
        }
    }
    while i > index && is_trailing_url_punct(cps[i - 1].value) {
        i -= 1;
    }
    (saw_at && saw_dot_after_at && i > index).then_some(i)
}

fn atoms(text: &str) -> Vec<Atom<'_>> {
    let cps = codepoints(text);
    let mut out = Vec::new();
    let mut i = 0;
    while i < cps.len() {
        if is_space(cps[i].value) {
            i += 1;
            continue;
        }
        let begin = i;
        let kind;
        if let Some(stop) = legal_number_atom_stop(&cps, i).or_else(|| web_atom_stop(text, &cps, i))
        {
            kind = AtomType::Other;
            i = stop;
        } else if is_uk(cps[i].value) {
            kind = AtomType::Uk;
            while i < cps.len() && (is_uk(cps[i].value) || is_inner_uk_apostrophe(&cps, i)) {
                i += 1;
            }
        } else if is_latin(cps[i].value) {
            kind = AtomType::Lat;
            while i < cps.len() && is_latin(cps[i].value) {
                i += 1;
            }
        } else if is_digit(cps[i].value) {
            kind = AtomType::Int;
            while i < cps.len() && is_digit(cps[i].value) {
                i += 1;
            }
        } else {
            kind = if is_token_punct(cps[i].value) { AtomType::Punct } else { AtomType::Other };
            i += 1;
        }
        let (start, stop) = (cps[begin].start, cps[i - 1].stop);
        out.push(Atom { start, stop, kind, text: &text[start..stop] });
    }
    out
}

fn is_smile(text: &str) -> bool {
    smile_at(text, 0) == Some(text.len())
}

/// Decides whether two adjacent atoms belong to the same token.
fn token_join(
    left_1: &Atom<'_>,
    left_2: Option<&Atom<'_>>,
    delimiter: &str,
    right_1: &Atom<'_>,
    right_2: Option<&Atom<'_>>,
    buffer: &str,
) -> bool {
    // Joins `a<delim>b` where `delim` is either the gap between the atoms or an
    // atom of its own, in which case the operands are the atoms beyond it.
    let around = |delim: char, pred: &dyn Fn(&Atom<'_>, &Atom<'_>) -> bool| -> bool {
        if delimiter.starts_with(delim) {
            return pred(left_1, right_1);
        }
        if !delimiter.is_empty() {
            return false;
        }
        if let Some(left_2) = left_2.filter(|_| left_1.text.starts_with(delim)) {
            return pred(left_2, right_1);
        }
        if let Some(right_2) = right_2.filter(|_| right_1.text.starts_with(delim)) {
            return pred(left_1, right_2);
        }
        false
    };

    let not_punct =
        |l: &Atom<'_>, r: &Atom<'_>| l.kind != AtomType::Punct && r.kind != AtomType::Punct;
    let both_int = |l: &Atom<'_>, r: &Atom<'_>| l.kind == AtomType::Int && r.kind == AtomType::Int;

    // `тест-драйв`, `snake_case`
    if "‑–—−-_".chars().any(|d| around(d, &not_punct)) {
        return true;
    }
    // `1.5`, `1,5`, `1/2`
    if ".,/\\".chars().any(|d| around(d, &both_int)) {
        return true;
    }

    if left_1.kind == AtomType::Punct && right_1.kind == AtomType::Punct {
        if is_smile(&format!("{buffer}{}", right_1.text)) {
            return true;
        }
        // `?!`, `...`
        if ".?!…".contains(left_1.text) && ".?!…".contains(right_1.text) {
            return true;
        }
        if left_1.text == right_1.text && (left_1.text == "-" || left_1.text == "*") {
            return true;
        }
    }

    // Web atoms absorb whatever touches them.
    if left_1.kind == AtomType::Other
        && matches!(right_1.kind, AtomType::Other | AtomType::Uk | AtomType::Lat)
    {
        return true;
    }
    if matches!(left_1.kind, AtomType::Other | AtomType::Uk | AtomType::Lat)
        && right_1.kind == AtomType::Other
    {
        return true;
    }

    // `5кг`
    if delimiter.is_empty()
        && left_1.kind == AtomType::Int
        && matches!(right_1.kind, AtomType::Uk | AtomType::Lat)
        && abbrev::is_known(&lower_ascii_ukrainian(right_1.text))
    {
        return true;
    }

    right_1.text == "!" && lower_ascii_ukrainian(left_1.text) == "yahoo"
}

/// Splits `text` into tokens.
#[must_use]
pub fn tokenize(text: &str) -> Vec<Substring<'_>> {
    let atoms = atoms(text);
    let Some(first) = atoms.first() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut start = first.start;
    let mut stop = first.stop;
    for i in 1..atoms.len() {
        let delimiter = &text[atoms[i - 1].stop..atoms[i].start];
        let buffer = &text[start..stop];
        let join = delimiter.is_empty()
            && token_join(
                &atoms[i - 1],
                atoms.get(i.wrapping_sub(2)),
                delimiter,
                &atoms[i],
                atoms.get(i + 1),
                buffer,
            );
        if join {
            stop = atoms[i].stop;
        } else {
            push_substring(&mut out, text, start, stop, false);
            start = atoms[i].start;
            stop = atoms[i].stop;
        }
    }
    push_substring(&mut out, text, start, stop, false);
    out
}
