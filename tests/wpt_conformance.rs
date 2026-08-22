//! Conformance regression tests curated from two device-independent,
//! `testharness.js`-based web-platform-tests (WPT) files. See
//! `plan/05-conformance.md` for the full research/selection rationale
//! (why these two files and not the rest of `css/mediaqueries/`, which
//! are visual reftests whose pass/fail signal depends on the executing
//! device/viewport and is therefore a "matching" concern, not a "parse"
//! concern — out of scope for this crate, see `CLAUDE.md`).
//!
//! Sources (retrieved 2026-08-22):
//!
//! - <https://github.com/web-platform-tests/wpt/blob/master/css/mediaqueries/test_media_queries.html>
//!   ("Media Queries Self-Contained Test Suite", authors: L. David
//!   Baron, Anne van Kesteren, Ms2ger). The helper functions
//!   `query_should_be_parseable(q)`/`query_should_not_be_parseable(q)`
//!   (lines 79–85 of the file) put a string into an isolated
//!   `@media screen, <q> {}` rule and check
//!   `sheet.cssRules[0].media.mediaText != "screen, not all"` to decide
//!   whether `<q>` parsed as its own `<media-query>` list entry — a
//!   pure grammar question, independent of the executing device.
//! - <https://github.com/web-platform-tests/wpt/blob/master/css/mediaqueries/mq-invalid-media-type-005.html>
//!   (author: Florian Rivoal). Checks
//!   `document.styleSheets[0].cssRules[i].conditionText === "not all"`
//!   for eight `@media` preludes that misuse reserved keywords
//!   (`not`/`and`/`or`/`only`) as a purported media type.
//!
//! All input strings below are copied verbatim from these two files;
//! only the WPT-internal test names are translated into Rust test
//! function names. The cited line numbers are current as of the
//! retrieval date above and may drift if the upstream files are
//! restructured — the copied strings and their expected parse
//! outcomes are unaffected by that drift, since they were copied by
//! value, not by reference.

use media_query_parse::parse_media_query;

// --- `test_media_queries.html` — `query_should_be_parseable` ---

#[test]
fn wpt_query_should_be_parseable_orientation() {
    // Line 112.
    assert!(parse_media_query("(orientation)").is_ok());
}

#[test]
fn wpt_query_should_be_parseable_not_orientation() {
    // Line 113.
    assert!(parse_media_query("not (orientation)").is_ok());
}

#[test]
fn wpt_query_should_be_parseable_all_and_orientation() {
    // Line 117.
    assert!(parse_media_query("all and (orientation)").is_ok());
}

#[test]
fn wpt_query_should_be_parseable_not_all_and_orientation() {
    // Line 118.
    assert!(parse_media_query("not all and (orientation)").is_ok());
}

#[test]
fn wpt_query_should_be_parseable_only_all_and_orientation() {
    // Line 119.
    assert!(parse_media_query("only all and (orientation)").is_ok());
}

#[test]
fn wpt_query_should_be_parseable_orientation_and_orientation() {
    // Line 122.
    assert!(parse_media_query("(orientation) and (orientation)").is_ok());
}

#[test]
fn wpt_query_should_be_parseable_orientation_or_orientation() {
    // Line 123.
    assert!(parse_media_query("(orientation) or (orientation)").is_ok());
}

#[test]
fn wpt_query_should_be_parseable_nested_or_and_or_not() {
    // Line 124.
    assert!(
        parse_media_query(
            "(orientation) or ((orientation) and ((orientation) or (orientation) or (not (orientation))))"
        )
        .is_ok()
    );
}

#[test]
fn wpt_query_should_be_parseable_all_and_orientation_and_orientation() {
    // Line 130.
    assert!(parse_media_query("all and (orientation) and (orientation)").is_ok());
}

#[test]
fn wpt_query_should_be_parseable_not_unknown_function() {
    // Line 710. Trailing space is part of the original WPT string.
    assert!(parse_media_query("not unknown(width) ").is_ok());
}

// --- `test_media_queries.html` — `query_should_not_be_parseable` ---

#[test]
fn wpt_query_should_not_be_parseable_only_orientation() {
    // Line 116. `only` requires a `<media-type>`, not a `<media-condition>`.
    assert!(parse_media_query("only (orientation)").is_err());
}

#[test]
fn wpt_query_should_not_be_parseable_not_not_orientation() {
    // Line 121. `not` is a reserved word, not a valid `<media-type>`;
    // as a second `<media-not>` it's missing the opening parenthesis.
    assert!(parse_media_query("not not (orientation)").is_err());
}

#[test]
fn wpt_query_should_not_be_parseable_all_and_orientation_or_orientation() {
    // Line 129. `<media-condition-without-or>` (after `<media-type> and`)
    // only allows `<media-and>*`, no `or`.
    assert!(parse_media_query("all and (orientation) or (orientation)").is_err());
}

#[test]
fn wpt_query_should_not_be_parseable_orientation_and_orientation_or_orientation() {
    // Line 132. `and`/`or` must not be mixed at the same level.
    assert!(parse_media_query("(orientation) and (orientation) or (orientation)").is_err());
}

#[test]
fn wpt_query_should_not_be_parseable_orientation_and_not_orientation() {
    // Line 133. After `and`, `<media-in-parens>` must follow, not `not (...)`.
    assert!(parse_media_query("(orientation) and not (orientation)").is_err());
}

#[test]
fn wpt_query_should_not_be_parseable_all_and_color_colon() {
    // Line 581. `color :` is not wrapped in `( … )` —
    // `<media-condition-without-or>` requires `<media-in-parens>`.
    assert!(parse_media_query("all and color :").is_err());
}

#[test]
fn wpt_query_should_not_be_parseable_all_and_color_colon_1() {
    // Line 582. Same as above, plus a missing parenthesis frame.
    assert!(parse_media_query("all and color : 1").is_err());
}

#[test]
fn wpt_query_should_not_be_parseable_screen_or_width() {
    // Line 693. `or` requires `<media-in-parens>` on both sides, not
    // `<media-type> or (...)`.
    assert!(parse_media_query("screen or (width)").is_err());
}

#[test]
fn wpt_query_should_not_be_parseable_screen_and_width_or_height() {
    // Line 694. Like line 129, plus a leading `<media-type>`.
    assert!(parse_media_query("screen and (width) or (height)").is_err());
}

#[test]
fn wpt_query_should_not_be_parseable_not_width_and_not_height() {
    // Line 708. `<media-not>` cannot be the left-hand side of an `and`
    // chain (`<media-condition-without-or> = <media-not> | <media-in-parens> <media-and>*`,
    // no `<media-not> <media-and>*` alternative).
    assert!(parse_media_query("not (width) and not (height)").is_err());
}

#[test]
fn wpt_query_should_not_be_parseable_not_not_width() {
    // Line 709. Same reasoning as line 121.
    assert!(parse_media_query("not not (width)").is_err());
}

// --- `mq-invalid-media-type-005.html` — reserved keywords as `<media-type>` ---

#[test]
fn wpt_invalid_media_type_not_and() {
    assert!(parse_media_query("not and").is_err());
}

#[test]
fn wpt_invalid_media_type_and() {
    assert!(parse_media_query("and").is_err());
}

#[test]
fn wpt_invalid_media_type_not_or() {
    assert!(parse_media_query("not or").is_err());
}

#[test]
fn wpt_invalid_media_type_or() {
    assert!(parse_media_query("or").is_err());
}

#[test]
fn wpt_invalid_media_type_not_not() {
    assert!(parse_media_query("not not").is_err());
}

#[test]
fn wpt_invalid_media_type_not() {
    assert!(parse_media_query("not").is_err());
}

#[test]
fn wpt_invalid_media_type_not_only() {
    assert!(parse_media_query("not only").is_err());
}

#[test]
fn wpt_invalid_media_type_only() {
    assert!(parse_media_query("only").is_err());
}
