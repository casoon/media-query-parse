//! AST types for the Media Queries Level 4 grammar (`<media-query-list>`
//! and below), as parsed by [`crate::parser`].
//!
//! Syntax and structure only — see `CLAUDE.md`: this crate has no "matches
//! a real device/viewport" concept.
//!
//! Grammar reference: [Media Queries Level 4][spec] §3, which already
//! defines the `<mf-range>` alternative (comparison operators, one- and
//! two-sided ranges) normatively alongside `<mf-plain>`/`<mf-boolean>`
//! — see `plan/DECISIONS.md` for why this is Level 4, not Level 5.
//! [`MediaFeature`] covers all three.
//!
//! [spec]: https://www.w3.org/TR/mediaqueries-4/

use crate::tokenizer::Token;

/// `<media-query-list> = <media-query>#`
///
/// Holds a list of *successfully parsed* queries. The public parsing
/// entry point ([`crate::parser::parse_media_query_list`]) returns a
/// `Result` per list entry instead of this type, since one invalid
/// entry must not fail the whole list (see `plan/DECISIONS.md` for why)
/// — `MediaQueryList` is here for callers that already have a fully
/// valid list in hand (e.g. after filtering out the `Err`s themselves).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct MediaQueryList(pub Vec<MediaQuery>);

/// `<media-query> = <media-condition>
///                | [ not | only ]? <media-type> [ and <media-condition-without-or> ]?`
///
/// Modeled as an enum rather than a single struct with optional fields:
/// the two grammar branches allow structurally different condition
/// kinds (a full `<media-condition>` with `or` only in the first
/// branch, `<media-condition-without-or>` only in the second), which an
/// enum expresses type-safely.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum MediaQuery {
    /// The bare `<media-condition>` branch.
    Condition(MediaCondition),
    /// The `[ not | only ]? <media-type> [ and <media-condition-without-or> ]?` branch.
    TypeQuery {
        /// The optional `not`/`only` prefix, if present.
        modifier: Option<MediaModifier>,
        /// The `<media-type>` itself.
        media_type: MediaType,
        /// The optional `and <media-condition-without-or>` suffix.
        condition: Option<MediaConditionWithoutOr>,
    },
}

/// `not` | `only`, as used in the `<media-type>` branch of `<media-query>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MediaModifier {
    /// `not`: negates the whole `<media-query>`.
    Not,
    /// `only`: present only to hide the query from legacy UAs that
    /// don't support media types other than the four originally
    /// defined ones; has no effect on this crate's parsing result.
    Only,
}

/// `<media-type> = <ident>`, structurally excluding `not`/`and`/`or`/
/// `only`/`layer` (spec §3: "The `<media-type>` production does not
/// include the keywords `only`, `not`, `and`, `or`, and `layer`").
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct MediaType(pub String);

/// `<media-condition> = <media-not> | <media-in-parens> [ <media-and>* | <media-or>* ]`
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum MediaCondition {
    /// `<media-not> = not <media-in-parens>`
    Not(MediaInParens),
    /// `<media-in-parens> <media-and>*`. A single element represents
    /// the bare `<media-in-parens>` case (zero `and`s).
    And(Vec<MediaInParens>),
    /// `<media-in-parens> <media-or>*` (at least 2 elements — a bare
    /// `<media-in-parens>` is always represented as `And` above).
    Or(Vec<MediaInParens>),
}

/// `<media-condition-without-or> = <media-not> | <media-in-parens> <media-and>*`
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum MediaConditionWithoutOr {
    /// `<media-not> = not <media-in-parens>`
    Not(MediaInParens),
    /// `<media-in-parens> <media-and>*`. A single element represents
    /// the bare `<media-in-parens>` case (zero `and`s).
    And(Vec<MediaInParens>),
}

/// `<media-in-parens> = ( <media-condition> ) | ( <media-feature> ) | <general-enclosed>`
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum MediaInParens {
    /// `( <media-condition> )`
    Condition(Box<MediaCondition>),
    /// `( <media-feature> )`
    Feature(MediaFeature),
    /// `<general-enclosed>`, the forward-compatibility fallback (see
    /// [`GeneralEnclosed`]).
    GeneralEnclosed(GeneralEnclosed),
}

/// `<media-feature> = [ <mf-plain> | <mf-boolean> | <mf-range> ]`
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum MediaFeature {
    /// `<mf-boolean> = <mf-name>`
    Boolean(MfName),
    /// `<mf-plain> = <mf-name> : <mf-value>`
    Plain {
        /// The `<mf-name>` (feature name) on the left of `:`.
        name: MfName,
        /// The `<mf-value>` on the right of `:`.
        value: MfValue,
    },
    /// `<mf-range>`, see [`MfRange`].
    Range(MfRange),
}

/// `<mf-name> = <ident>`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct MfName(pub String);

/// `<mf-value> = <number> | <dimension> | <ident> | <ratio>`
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum MfValue {
    /// `<number>`
    Number(f64),
    /// `<dimension>`
    Dimension {
        /// The numeric part.
        value: f64,
        /// The unit identifier (e.g. `px`).
        unit: String,
    },
    /// `<ident>`
    Ident(String),
    /// `<ratio> = <number [0,∞]> <number [0,∞]>`, restricted here to
    /// non-negative integers, matching typical media-feature usage
    /// (e.g. `(aspect-ratio: 16/9)`).
    Ratio {
        /// The number before `/`.
        numerator: u32,
        /// The number after `/`.
        denominator: u32,
    },
}

/// `<mf-range>`, all four grammar alternatives:
///
/// ```text
/// <mf-range> = <mf-name> <mf-comparison> <mf-value>
///            | <mf-value> <mf-comparison> <mf-name>
///            | <mf-value> <mf-lt> <mf-name> <mf-lt> <mf-value>
///            | <mf-value> <mf-gt> <mf-name> <mf-gt> <mf-value>
/// ```
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum MfRange {
    /// `<mf-name> <mf-comparison> <mf-value>`
    NameFirst {
        /// The `<mf-name>` on the left.
        name: MfName,
        /// The comparison operator between name and value.
        operator: MfComparison,
        /// The `<mf-value>` on the right.
        value: MfValue,
    },
    /// `<mf-value> <mf-comparison> <mf-name>`
    ValueFirst {
        /// The `<mf-value>` on the left.
        value: MfValue,
        /// The comparison operator between value and name.
        operator: MfComparison,
        /// The `<mf-name>` on the right.
        name: MfName,
    },
    /// `<mf-value> <mf-lt> <mf-name> <mf-lt> <mf-value>`
    ///  | `<mf-value> <mf-gt> <mf-name> <mf-gt> <mf-value>`
    ///
    /// One direction (`<mf-lt>` family or `<mf-gt>` family) for both
    /// operators, never a mixed form — the grammar itself only lists
    /// these two alternatives, no `<mf-lt> ... <mf-gt>` mix and no
    /// `<mf-eq>` in this position. Modeled with `direction` plus two
    /// separate inclusive flags rather than two independent
    /// `MfComparison` fields, so that a mixed form isn't representable
    /// in the type at all.
    Interval {
        /// The `<mf-value>` bound on the left.
        lower: MfValue,
        /// Whether the left operator is inclusive (`<=`/`>=`) rather
        /// than strict (`<`/`>`).
        lower_inclusive: bool,
        /// The `<mf-name>` in the middle.
        name: MfName,
        /// Whether the right operator is inclusive (`<=`/`>=`) rather
        /// than strict (`<`/`>`).
        upper_inclusive: bool,
        /// The `<mf-value>` bound on the right.
        upper: MfValue,
        /// Which operator family (`<mf-lt>` or `<mf-gt>`) both sides use.
        direction: MfRangeDirection,
    },
}

/// Direction of a two-sided `<mf-range>`: `Ascending` is the `<mf-lt>`
/// family (`lower < name < upper`), `Descending` is the `<mf-gt>`
/// family (`lower > name > upper`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MfRangeDirection {
    /// The `<mf-lt>` family (`lower < name < upper`, or `<=`).
    Ascending,
    /// The `<mf-gt>` family (`lower > name > upper`, or `>=`).
    Descending,
}

/// `<mf-comparison> = <mf-lt> | <mf-gt> | <mf-eq>`, for the one-sided
/// `<mf-range>` forms ([`MfRange::NameFirst`]/[`MfRange::ValueFirst`]).
/// `<mf-lt> = '<' '='?`, `<mf-gt> = '>' '='?`, `<mf-eq> = '='`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MfComparison {
    /// `<` (strictly less than)
    Lt,
    /// `<=` (less than or equal to)
    Le,
    /// `>` (strictly greater than)
    Gt,
    /// `>=` (greater than or equal to)
    Ge,
    /// `=` (equal to)
    Eq,
}

/// `<general-enclosed>`: a syntactically well-bracketed but not
/// further interpretable block, kept verbatim as a forward-
/// compatibility fallback (spec §3, prose right after the grammar) —
/// not a parse error. Holds the raw tokens of the block's content: for
/// the `( <any-value>? )` alternative, the tokens between the
/// parentheses (parentheses themselves excluded); for the
/// `<function-token> <any-value>? )` alternative, the function token
/// followed by its argument tokens (closing `)` excluded). See
/// `crate::parser`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct GeneralEnclosed {
    /// The raw tokens of the block's content, see the type-level doc
    /// comment above for exactly which tokens are included/excluded.
    pub tokens: Vec<Token>,
}
