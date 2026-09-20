//! Ukrainian text normalization, tokenization and sentence splitting.
//!
//! The crate is split into two independent halves:
//!
//! * [`rozpodil`] segments text into sentences and tokens, returning borrowed
//!   [`Substring`](rozpodil::Substring) slices with byte offsets into the input.
//! * [`uktextnorm`] rewrites numbers, dates, units, currencies, abbreviations and
//!   other machine-readable spellings into the words a Ukrainian speaker would say.
//!
//! ```
//! use normalize_uk::{rozpodil, uktextnorm};
//!
//! assert_eq!(uktextnorm::number_to_words(123), "сто двадцять три");
//! let sentences = rozpodil::split_sentences("Перше речення. Друге.");
//! assert_eq!(sentences.len(), 2);
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]
// These pedantic style lints conflict with deliberate crate design: the normalization
// passes keep their regexes beside the transformation that uses them, several passes
// are clearest as linear pipelines, and the public options type exposes independent
// feature switches rather than an artificial state machine.
#![allow(clippy::items_after_statements)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::too_many_lines)]

pub mod rozpodil;
pub mod uktextnorm;

/// The README, compiled as a doctest so its examples cannot drift from the API.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct Readme;
