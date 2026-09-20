//! Sentence and token segmentation for Ukrainian text.
//!
//! Both entry points borrow from the input and report byte offsets, so
//! `&text[span.start..span.stop] == span.text` always holds.
//!
//! ```
//! use normalize_uk::rozpodil::{split_sentences, tokenize};
//!
//! let text = "Це тест. І ще один!";
//! assert_eq!(
//!     split_sentences(text).iter().map(|s| s.text).collect::<Vec<_>>(),
//!     ["Це тест.", "І ще один!"],
//! );
//! assert_eq!(tokenize("П'ять зв'язків.").len(), 3);
//! ```

mod abbrev;
mod chars;
mod sentences;
mod tokens;

pub use sentences::split_sentences;
pub use tokens::tokenize;

/// A slice of the input together with its byte offsets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Substring<'a> {
    /// Byte offset of the first character.
    pub start: usize,
    /// Byte offset one past the last character.
    pub stop: usize,
    /// The borrowed text, equal to `&source[start..stop]`.
    pub text: &'a str,
}

/// Pushes `text[start..stop]` onto `out`, optionally trimming surrounding
/// whitespace first and skipping the span when nothing is left.
fn push_substring<'a>(
    out: &mut Vec<Substring<'a>>,
    text: &'a str,
    start: usize,
    stop: usize,
    trim: bool,
) {
    let mut start = start;
    let mut view = &text[start..stop];
    if trim {
        let (offset, trimmed) = sentences::trim_view(view);
        start += offset;
        view = trimmed;
    }
    if !view.is_empty() {
        out.push(Substring { start, stop: start + view.len(), text: view });
    }
}
