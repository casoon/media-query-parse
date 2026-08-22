//! A pure-Rust parser for the CSS Media Queries grammar.
//!
//! Tokenizer (CSS Syntax Level 3) plus a Media Queries Level 4
//! grammar/AST/parser, including the `<mf-range>` comparison syntax.
//! See `plan/` for the roadmap.
//!
//! # Example
//!
//! ```
//! use media_query_parse::{parse_media_query, parse_media_query_list};
//!
//! // A single, syntactically valid media query.
//! assert!(parse_media_query("screen and (min-width: 400px)").is_ok());
//!
//! // An invalid media query reports a `ParseError` instead of panicking:
//! // `only` requires a following `<media-type>`, which is missing here.
//! assert!(parse_media_query("only").is_err());
//!
//! // `parse_media_query_list` splits a comma-separated list and parses
//! // each entry independently, returning one `Result` per entry.
//! let results = parse_media_query_list("screen, only");
//! assert!(results[0].is_ok());
//! assert!(results[1].is_err());
//! ```

#![deny(missing_docs)]

pub mod ast;
pub mod parser;
pub mod tokenizer;

pub use ast::{
    GeneralEnclosed, MediaCondition, MediaConditionWithoutOr, MediaFeature, MediaInParens,
    MediaModifier, MediaQuery, MediaQueryList, MediaType, MfComparison, MfName, MfRange,
    MfRangeDirection, MfValue,
};
pub use parser::{ParseError, parse_media_query, parse_media_query_list};
