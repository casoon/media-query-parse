# media-query-parse

A pure-Rust implementation of the [CSS Media Queries](https://www.w3.org/TR/mediaqueries-5/)
grammar — parses a media query or media condition string and reports
whether it is syntactically valid, without evaluating it against any
actual device/viewport state.

Generic and standalone — not tied to HTML, a specific host language, or
any particular consumer. This crate only parses the grammar; it does not
decide whether a query *matches* anything.

## Status

Feature-complete: tokenizer (CSS Syntax Level 3) and the full Media
Queries Level 4 grammar, including the `<mf-range>` comparison syntax
(`(400px <= width <= 700px)`). Verified against 29 external
web-platform-tests conformance cases. Public API is documented
(`#![deny(missing_docs)]`) and marked `#[non_exhaustive]` where the
grammar may grow.

Published on crates.io as [`media-query-parse`](https://crates.io/crates/media-query-parse).

## Usage

```rust
use media_query_parse::{parse_media_query, parse_media_query_list};

// A single, syntactically valid media query.
assert!(parse_media_query("screen and (min-width: 400px)").is_ok());

// An invalid media query reports a `ParseError` instead of panicking:
// `only` requires a following `<media-type>`, which is missing here.
assert!(parse_media_query("only").is_err());

// `parse_media_query_list` splits a comma-separated list and parses
// each entry independently, returning one `Result` per entry.
let results = parse_media_query_list("screen, only");
assert!(results[0].is_ok());
assert!(results[1].is_err());
```

## License

MIT — see [LICENSE](LICENSE).
