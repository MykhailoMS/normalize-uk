# normalize-uk

[![CI](https://github.com/RustedBytes/normalize-uk/actions/workflows/ci.yml/badge.svg)](https://github.com/RustedBytes/normalize-uk/actions/workflows/ci.yml)

Ukrainian text normalization, tokenization and sentence splitting, in Rust.

The crate turns machine-readable spellings into the words a Ukrainian speaker
would say — numbers, dates, times, ranges, units, currencies, abbreviations,
identifiers and web addresses — and splits text into sentences and tokens.

```toml
[dependencies]
normalize-uk = "0.5"
```

```rust
use normalize_uk::{rozpodil, uktextnorm};

assert_eq!(uktextnorm::normalize("5 кг"), "п'ять кілограмів");
assert_eq!(uktextnorm::number_to_words(123), "сто двадцять три");

let sentences = rozpodil::split_sentences("Привіт! Це тест.");
assert_eq!(sentences.iter().map(|s| s.text).collect::<Vec<_>>(), ["Привіт!", "Це тест."]);
```

## Normalization

`normalize` applies the default options. Use `normalize_preset` for a named
bundle, or `normalize_with` for full control.

```rust
use normalize_uk::uktextnorm::{normalize, normalize_preset, normalize_with,
                               NormalizeOptions, NormalizePreset, RangeStyle};

assert_eq!(normalize("01.05.2024"), "перше травня дві тисячі двадцять четвертого року");

// Presets bundle the options for a use case.
let spoken = normalize_preset("5–7 кг", NormalizePreset::TtsFriendly);
assert_eq!(spoken, "від п'яти до семи кілограмів");

// Or start from a preset and adjust.
let options = NormalizeOptions {
    range_style: RangeStyle::Compact,
    ..NormalizeOptions::preset(NormalizePreset::TtsFriendly)
};
assert_eq!(normalize_with("5–7 кг", &options), "п'ять сім кілограмів");
```

The presets are `Default`, `TtsFriendly` (everything spelled out, for speech
synthesis), `Conservative` (changes as little as possible) and
`SearchIndexing` (keeps tokens searchable rather than speakable).

### Ambiguity controls

`NormalizeOptions` keeps backwards-compatible defaults while letting callers
resolve ambiguous input explicitly:

- `colon_style` — contextual clock/ratio detection, forced clock, or forced ratio.
- `numeric_date_order` — day-month-year, month-day-year, or preserving dates
  where both fields are at most 12.
- `currency_symbol_policy` — assume the common currency for `$` and `¥`, or
  preserve those ambiguous symbols.

### Reporting what had to be guessed

`flag_uncertain` reports every place the reading involved a judgement call.
`flag_uncertain_with` takes options and omits the warnings they already
resolve; invalid-value diagnostics always remain.

```rust
use normalize_uk::uktextnorm::flag_uncertain;

let spans = flag_uncertain("Дата 30.02.2024");
assert!(spans.iter().any(|s| s.text == "30.02.2024"));
```

`UncertainSpan::start` and `stop` are byte offsets, so
`&source[span.start..span.stop] == span.text`.

### Tolerating ASR-distorted input

Speech recognition rarely hands the normalizer a clean token: the same word
arrives with a missing apostrophe (`дев'ятнадцятого` → `девятнадцятого`), glued
or split (`ю ес бі` → `юесбі`), or in a surzhyk / phonetic variant. Under the
default `InputTolerance::Strict` such a token misses the exact lexicon lookups
and passes through unchanged. `InputTolerance::Asr` adds a fallback that runs
*only after an exact lookup misses*, resolving the token against the closed
lexicon in two cheap, deterministic stages — a canonical key that folds
separators and confusable spellings, then a bounded edit-distance match to the
single closest entry (ties are left unresolved rather than guessed).

```rust
use normalize_uk::uktextnorm::{
    flag_uncertain_with, normalize_with, InputTolerance, NormalizeOptions, UncertaintyCategory,
};

let options = NormalizeOptions { input_tolerance: InputTolerance::Asr, ..Default::default() };

// A recognizer typo still reaches its reading.
assert_eq!(normalize_with("spotifay", &options), "спотіфай");

// Every approximate reading is reported, never silently guessed.
let spans = flag_uncertain_with("spotifay", &options);
assert!(spans.iter().any(|s| s.category == UncertaintyCategory::ApproximateMatch));
```

The fallback never runs on the hot path for clean text, and the search space is
always a closed lexicon (hundreds of entries), never free text, so the
behaviour is deterministic and testable by the golden corpora.

The same fallback also runs over Cyrillic input, where ASR distorts the
*reading itself* rather than a Latin spelling:

```rust
use normalize_uk::uktextnorm::{normalize_with, InputTolerance, NormalizeOptions};

let options = NormalizeOptions { input_tolerance: InputTolerance::Asr, ..Default::default() };

// "ватсап" is a one-edit distortion of the canonical reading "вотсап".
assert_eq!(normalize_with("ватсап", &options), "вотсап");
// A lowercased or phonetically-spelled acronym is restored, then expanded.
assert!(normalize_with("сума пдв", &options).contains("додану вартість"));
assert!(normalize_with("сума педеве", &options).contains("додану вартість"));
// Everyday Ukrainian prose is never dragged onto a reading or acronym.
assert_eq!(normalize_with("сьогодні я пив каву", &options), "сьогодні я пив каву");
```

The closed target sets are foreign-shaped by design — brand/English readings
and acronym keys (plus their phonetic letter-name spellings, so `педеве` folds
back to `ПДВ`). The canonical key is *phonetic*: it folds the confusions a
Ukrainian recognizer actually makes (`і`/`ї`/`и`, `е`/`є`, `я`→`а`, `ю`→`у`,
`ґ`→`г`, the soft sign, doublings, and Russian/surzhyk carry-over), so
near-homophones collapse before any edit-distance step.

Inflected ordinary words are not repaired by default, because fuzzy-matching
open prose against itself would corrupt it. The universal extension point is
`asr_vocabulary`: hand the normalizer any list of canonical Ukrainian words — a
domain glossary or a full lexicon — and the *same* phonetic-key and
bounded-edit rules repair distorted tokens against it.

```rust
use normalize_uk::uktextnorm::{normalize_with, InputTolerance, NormalizeOptions};

let options = NormalizeOptions {
    input_tolerance: InputTolerance::Asr,
    asr_vocabulary: vec!["автентифікація".to_owned(), "ідентифікатор".to_owned()],
    ..Default::default()
};

assert_eq!(normalize_with("автентіфікація", &options), "автентифікація");
```

Load the list from a one-column `word` TSV with `load_asr_vocabulary_tsv`.

A recognizer also splits or glues multi-word targets (`вай фай` / `вайфай` for
`вай-фай`). Because the phonetic key drops separators, a split window of tokens
and its glued form share one key, so a sliding-window pass rejoins either shape
to the canonical target — including a multi-word entry supplied via
`asr_vocabulary`:

```rust
# use normalize_uk::uktextnorm::{normalize_with, InputTolerance, NormalizeOptions};
let options = NormalizeOptions {
    input_tolerance: InputTolerance::Asr,
    asr_vocabulary: vec!["вай-фай".to_owned()],
    ..Default::default()
};
assert!(normalize_with("увімкни вай фай", &options).contains("вай-фай")); // split
assert!(normalize_with("увімкни вайфай", &options).contains("вай-фай")); // glued
```

## Numbers

```rust
use normalize_uk::uktextnorm::{number_to_ordinal_words, number_to_words,
                               number_to_words_case, number_to_words_digit_by_digit,
                               GrammaticalCase, OrdinalForm};

// A bare "одна" before "тисяча" is dropped, as Ukrainian usage requires.
assert_eq!(number_to_words(1_234), "тисяча двісті тридцять чотири");
assert_eq!(number_to_words(2_234), "дві тисячі двісті тридцять чотири");
assert_eq!(number_to_ordinal_words(21, OrdinalForm::NomF), "двадцять перша");
assert_eq!(number_to_words_case(500, GrammaticalCase::Genitive), "п'ятисот");
assert_eq!(number_to_words_digit_by_digit("007"), "нуль нуль сім");
```

Values above `MAX_SPELLED_NUMBER` (`999_999_999_999_999_999`) are read digit by
digit instead of spelled out.

## Segmentation

Both entry points borrow from the input and report byte offsets, so
`&text[span.start..span.stop] == span.text` always holds.

```rust
use normalize_uk::rozpodil::{split_sentences, tokenize};

let text = "м. Київ, вул. Хрещатик, 1. Зустріч о 10:30.";
assert_eq!(split_sentences(text).len(), 2);
assert_eq!(tokenize("П'ять зв'язків.").len(), 3);
```

## Custom vocabulary

Pass a map of preferred readings to override the built-in brand and
English-word lexicons for a single call. The `normalize_english_words` switch
also controls custom readings.

```rust
use normalize_uk::uktextnorm::{normalize_with, NormalizeOptions};

let options = NormalizeOptions {
    vocabulary: [("google", "гуголь"), ("acme", "акме")]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect(),
    ..NormalizeOptions::default()
};
assert_eq!(normalize_with("Google і Acme", &options), "гуголь і акме");
```

Readings can also be loaded from a UTF-8 TSV file with the same columns as
`data/lexicons/brands.tsv`:

```text
latin	cyrillic
Acme	акме
Google	гуголь
```

```rust,no_run
use normalize_uk::uktextnorm::load_vocabulary_tsv;

let words = load_vocabulary_tsv("my_words.tsv")?;
# Ok::<(), normalize_uk::uktextnorm::VocabularyError>(())
```

Latin keys are single ASCII words, matched without regard to case.

## Currency and cryptocurrency coverage

Normalization covers 178 ISO 4217 List One codes from the 2026-01-01 data
snapshot, including their 0-, 2-, 3- and 4-digit minor-unit rules. More than 70
common cryptocurrency and finance tickers have natural Ukrainian readings.
Other 2–10 character uppercase alphanumeric tickers are spelled out after
amounts and when paired with a known asset, so newly introduced assets do not
require an immediate release. Prefix and suffix amounts, localized thousands
separators, signs, decimals and the `₿` symbol are all supported.

## Lexicons

The lexicons under `data/lexicons/` are the source of truth. They are embedded
at compile time with `include_str!` and parsed once on first use, so editing a
TSV and rebuilding is all that is needed to change a reading. `cargo test`
checks each table for duplicate keys, out-of-range values and empty columns.

## Development

```sh
cargo test
cargo clippy --all-targets
cargo fmt --all
```

The test suite has four parts:

- `tests/conformance.rs` — the reference assertion suite, reporting every
  failure at once rather than stopping at the first.
- `tests/golden.rs` — the TSV corpora under `tests/data/`, including an
  idempotence check on the sentence corpus.
- `tests/robustness.rs` — awkward input must not panic, empty the text, or
  produce spans that disagree with the source.
- `tests/rozpodil.rs` and `tests/vocabulary.rs` — segmentation and vocabulary
  loading.

## License

MIT. See [LICENSE](LICENSE).
