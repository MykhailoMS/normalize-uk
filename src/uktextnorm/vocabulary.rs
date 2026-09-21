//! Loading user-supplied word readings from a TSV file.

use std::collections::HashMap;
use std::fmt;
use std::path::Path;

/// Why a vocabulary file could not be loaded.
#[derive(Debug)]
pub enum VocabularyError {
    /// The file could not be read.
    Io(std::io::Error),
    /// A line did not have the expected shape.
    Invalid {
        /// The 1-based line number, or 0 when the problem is the file as a whole.
        line: usize,
        /// What was wrong.
        message: &'static str,
    },
}

impl fmt::Display for VocabularyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "cannot read vocabulary file: {e}"),
            Self::Invalid { line: 0, message } => write!(f, "vocabulary file {message}"),
            Self::Invalid { line, message } => write!(f, "vocabulary line {line}: {message}"),
        }
    }
}

impl std::error::Error for VocabularyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Invalid { .. } => None,
        }
    }
}

impl From<std::io::Error> for VocabularyError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// True when `word` is a single ASCII Latin word, optionally hyphenated.
fn is_latin_word(word: &str) -> bool {
    let bytes = word.as_bytes();
    match (bytes.first(), bytes.last()) {
        (Some(first), Some(last)) if first.is_ascii_alphabetic() && last.is_ascii_alphabetic() => {}
        _ => return false,
    }
    bytes.iter().all(|b| b.is_ascii_alphabetic() || *b == b'-' || *b == b'\'')
}

/// Reads a `latin<TAB>cyrillic` TSV file of preferred readings.
///
/// The file must start with a `latin\tcyrillic` header; blank lines and lines
/// beginning with `#` are ignored. Keys are lowercased, so lookups ignore case.
///
/// # Errors
///
/// Returns an error when the file cannot be read, the header is missing, or a
/// row has the wrong number of columns, an invalid key, or a blank reading.
pub fn load_vocabulary_tsv(
    path: impl AsRef<Path>,
) -> Result<HashMap<String, String>, VocabularyError> {
    parse_vocabulary(&std::fs::read_to_string(path)?)
}

/// Parses the contents of a vocabulary TSV file.
///
/// # Errors
///
/// As [`load_vocabulary_tsv`], minus the I/O cases.
pub fn parse_vocabulary(source: &str) -> Result<HashMap<String, String>, VocabularyError> {
    let mut words = HashMap::new();
    let mut header_seen = false;
    for (index, raw) in source.lines().enumerate() {
        let line_number = index + 1;
        let mut line = raw.strip_suffix('\r').unwrap_or(raw);
        if line_number == 1 {
            line = line.strip_prefix('\u{feff}').unwrap_or(line);
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let invalid = |message| VocabularyError::Invalid { line: line_number, message };
        if !header_seen {
            if line != "latin\tcyrillic" {
                return Err(invalid("expected latin<TAB>cyrillic header"));
            }
            header_seen = true;
            continue;
        }
        let mut fields = line.split('\t');
        let (Some(key), Some(reading), None) = (fields.next(), fields.next(), fields.next()) else {
            return Err(invalid("expected two columns"));
        };
        if !is_latin_word(key) {
            return Err(invalid("latin must be one ASCII Latin word"));
        }
        if reading.is_empty() || reading.trim() != reading {
            return Err(invalid("cyrillic must be nonempty without surrounding whitespace"));
        }
        if words.insert(key.to_ascii_lowercase(), reading.to_owned()).is_some() {
            return Err(invalid("duplicate latin word"));
        }
    }
    if !header_seen {
        return Err(VocabularyError::Invalid {
            line: 0,
            message: "is missing latin<TAB>cyrillic header",
        });
    }
    Ok(words)
}

/// Reads a one-column `word` TSV of canonical Cyrillic words for the ASR
/// fallback to repair distorted tokens to (see
/// [`NormalizeOptions::asr_vocabulary`](super::NormalizeOptions)).
///
/// The file must start with a `word` header; blank lines and lines beginning
/// with `#` are ignored. Each remaining line is one canonical word.
///
/// # Errors
///
/// Returns an error when the file cannot be read, the header is missing, or a
/// row is empty or has more than one column.
pub fn load_asr_vocabulary_tsv(path: impl AsRef<Path>) -> Result<Vec<String>, VocabularyError> {
    parse_asr_vocabulary(&std::fs::read_to_string(path)?)
}

/// Parses the contents of an ASR-vocabulary TSV file.
///
/// # Errors
///
/// As [`load_asr_vocabulary_tsv`], minus the I/O cases.
pub fn parse_asr_vocabulary(source: &str) -> Result<Vec<String>, VocabularyError> {
    let mut words = Vec::new();
    let mut header_seen = false;
    for (index, raw) in source.lines().enumerate() {
        let line_number = index + 1;
        let mut line = raw.strip_suffix('\r').unwrap_or(raw);
        if line_number == 1 {
            line = line.strip_prefix('\u{feff}').unwrap_or(line);
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let invalid = |message| VocabularyError::Invalid { line: line_number, message };
        if !header_seen {
            if line != "word" {
                return Err(invalid("expected a `word` header"));
            }
            header_seen = true;
            continue;
        }
        if line.contains('\t') {
            return Err(invalid("expected a single column"));
        }
        if line.trim() != line || line.is_empty() {
            return Err(invalid("word must be nonempty without surrounding whitespace"));
        }
        words.push(line.to_owned());
    }
    if !header_seen {
        return Err(VocabularyError::Invalid { line: 0, message: "is missing a `word` header" });
    }
    Ok(words)
}
