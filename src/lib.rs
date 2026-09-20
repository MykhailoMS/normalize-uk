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

pub mod rozpodil;
pub mod uktextnorm;

/// The README, compiled as a doctest so its examples cannot drift from the API.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct Readme;
