//! Recursive-descent parser for the Media Queries Level 4 grammar,
//! built on top of the [`crate::tokenizer::Tokenizer`] from phase 02.
//!
//! One function per grammar production (see the doc comment on each
//! function below for the production it implements), mirroring the
//! structure of the grammar in `plan/03-grammar.md` §"Grammatik-
//! Produktionen".
//!
//! **Parsing approach — no tokenizer backtracking.** `<media-in-parens>`
//! has three alternatives behind the same opening `(` (or function
//! token). Instead of backtracking the main token stream, the content
//! of a parenthesized block is first consumed as one atomic token
//! block (analogous to "Consume a simple block", [CSS Syntax Level
//! 3][syntax-3] §5.4.8: all tokens up to the mirror closing token,
//! respecting nested brackets), then `<media-condition>` and
//! `<media-feature>` are tried in turn against that isolated token
//! slice. If both fail, the block is kept verbatim as
//! `<general-enclosed>` (see `ast::GeneralEnclosed`) — lookahead stays
//! confined to the bracket content instead of needing true backtracking
//! on the main stream.
//!
//! **Error handling.** Two distinct failure kinds exist per spec §3.2:
//! a syntax error a parser can't even parse as a block (e.g. unbalanced
//! parentheses) is a real error, represented by [`ParseError`] /
//! `Result`. A single `<media-query>` list entry that is syntactically
//! well-formed as a block but doesn't match the grammar (e.g. an
//! unknown media feature) is, per spec, a browser-matching concern (UAs
//! replace it with `not all`) — this crate has no "matches" concept
//! (see `CLAUDE.md`), so [`parse_media_query_list`] surfaces the actual
//! per-entry `Result` instead of silently rewriting it.
//!
//! [syntax-3]: https://www.w3.org/TR/css-syntax-3/

use crate::ast::{
    GeneralEnclosed, MediaCondition, MediaConditionWithoutOr, MediaFeature, MediaInParens,
    MediaModifier, MediaQuery, MediaType, MfComparison, MfName, MfRange, MfRangeDirection, MfValue,
};
use crate::tokenizer::{NumericType, Token, tokenize};

/// A real syntax error, distinct from a `<media-query>` that is
/// syntactically well-formed but doesn't match the grammar (see the
/// module doc comment). One variant per failure cause.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ParseError {
    /// A `(` (or a function's implicit `(`) was never closed before
    /// the input ended.
    UnbalancedParens,
    /// A `<media-query-list>` entry (or a `<media-in-parens>` block)
    /// had no tokens to parse.
    EmptyMediaQuery,
    /// Extra tokens remained after a production was fully parsed.
    TrailingTokens(Token),
    /// Expected a `<media-type>` (a plain `<ident>`).
    ExpectedMediaType(Option<Token>),
    /// `<media-type>` matched one of the reserved keywords
    /// `not`/`and`/`or`/`only`/`layer`, which the grammar excludes.
    ReservedMediaType(String),
    /// Expected `<media-in-parens>` (`(` or a function token).
    ExpectedMediaInParens(Option<Token>),
    /// Expected an `<ident>` for `<mf-name>`.
    ExpectedMfName(Option<Token>),
    /// Expected `:` between `<mf-name>` and `<mf-value>`.
    ExpectedColon(Option<Token>),
    /// Expected an `<mf-value>` (`<number>`/`<dimension>`/`<ident>`/`<ratio>`).
    ExpectedMfValue(Option<Token>),
    /// A `<ratio>` (`<number> / <number>`) was malformed.
    InvalidRatio,
    /// Expected an `<mf-comparison>` (`<`, `<=`, `>`, `>=`, or `=`).
    ExpectedMfComparison(Option<Token>),
    /// The second comparison operator of a two-sided `<mf-range>` was
    /// missing, from the wrong family (mixing `<mf-lt>`/`<mf-gt>`), or
    /// the first operator was `<mf-eq>` (`<mf-eq>` never appears in the
    /// two-sided form) — see the grammar note on [`crate::ast::MfRange`].
    InvalidMfRangeInterval,
    /// Expected a specific keyword (`not`/`and`/`or`).
    ExpectedKeyword {
        /// The keyword that was expected.
        keyword: &'static str,
        /// The token found instead, or `None` at end of input.
        found: Option<Token>,
    },
}

/// Maps an opening token to its mirror closing token, per [CSS Syntax
/// Level 3][spec] §5.4.8 ("Consume a simple block"). `None` if `token`
/// is not an opener.
///
/// [spec]: https://www.w3.org/TR/css-syntax-3/
fn matching_close(token: &Token) -> Option<Token> {
    match token {
        Token::OpenParen | Token::Function(_) => Some(Token::CloseParen),
        Token::OpenSquare => Some(Token::CloseSquare),
        Token::OpenCurly => Some(Token::CloseCurly),
        _ => None,
    }
}

fn is_ident_ci(token: Option<&Token>, keyword: &str) -> bool {
    matches!(token, Some(Token::Ident(s)) if s.eq_ignore_ascii_case(keyword))
}

/// A token paired with whether a [`Token::Whitespace`] token
/// immediately preceded it in the raw tokenizer output, before
/// whitespace tokens are stripped by [`prepare_tokens`]. Needed to
/// recognize `<mf-lt>`/`<mf-gt>` (`'<' '='?` / `'>' '='?`) as two
/// *directly adjacent* `Delim` tokens with no space between — after
/// whitespace tokens are stripped, `<=` and `< =` would otherwise both
/// tokenize to the same `Delim('<'), Delim('=')` pair and become
/// indistinguishable. See [`mf_comparison`] and `plan/DECISIONS.md`.
type PositionedToken = (Token, bool);

/// Cursor over a token slice for the recursive-descent parser. Plays
/// the role of the `Peekable<Tokenizer>` wrapper from `plan/03-
/// grammar.md`, specialized to a slice so that `<media-in-parens>`
/// content (already isolated via [`Parser::consume_block`]) can be
/// parsed the same way as a top-level `<media-query>`.
struct Parser<'a> {
    tokens: &'a [PositionedToken],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [PositionedToken]) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|(token, _)| token)
    }

    fn peek_at(&self, offset: usize) -> Option<&Token> {
        self.tokens.get(self.pos + offset).map(|(token, _)| token)
    }

    fn advance(&mut self) -> Option<Token> {
        let token = self.peek().cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn is_at_end(&self) -> bool {
        self.pos == self.tokens.len()
    }

    /// Whether the token at the current position was immediately
    /// preceded by whitespace in the original, unfiltered token
    /// stream. Used by [`mf_comparison`] to tell `<=`/`>=` (no
    /// whitespace between `<`/`>` and `=`) apart from `< =`/`> =` (an
    /// `<mf-lt>`/`<mf-gt>` operator immediately followed by an
    /// unrelated `<mf-eq>`).
    fn current_preceded_by_whitespace(&self) -> bool {
        self.tokens
            .get(self.pos)
            .map(|(_, preceded_by_ws)| *preceded_by_ws)
            .unwrap_or(false)
    }

    /// Consumes a bracketed block starting at the current position
    /// (which must be an opener: `(`, `[`, `{`, or a function token),
    /// returning the raw tokens strictly between the matching open/
    /// close (nested blocks of any kind are included whole). This is
    /// the "consume content as an atomic block" step described in the
    /// module doc comment — it never backtracks, it only fails if no
    /// matching closer is found before the input ends.
    fn consume_block(&mut self) -> Result<Vec<PositionedToken>, ParseError> {
        let opener = self.tokens[self.pos].0.clone();
        let mut closers = vec![matching_close(&opener).expect("caller checked an opener")];
        self.pos += 1;
        let mut inner = Vec::new();
        loop {
            match self.tokens.get(self.pos).cloned() {
                None => return Err(ParseError::UnbalancedParens),
                Some((token, preceded_by_ws)) => {
                    let closed_outermost = track_bracket_depth(&token, &mut closers);
                    self.pos += 1;
                    if closed_outermost {
                        return Ok(inner);
                    }
                    inner.push((token, preceded_by_ws));
                }
            }
        }
    }
}

/// Updates `closers` (a stack of pending closing tokens, innermost
/// last) for `token`, per [CSS Syntax Level 3][spec] §5.4.8's bracket-
/// nesting rule: opening tokens push their mirror closer, and a token
/// matching the innermost pending closer pops it. Returns `true` if
/// `token` closed the outermost bracket, i.e. `closers` just became
/// empty. Shared by [`Parser::consume_block`] (atomic `<media-in-
/// parens>` block consumption) and [`split_top_level_commas`]
/// (`<media-query-list>` splitting), which both need this same
/// nesting-depth tracking for otherwise unrelated purposes.
///
/// [spec]: https://www.w3.org/TR/css-syntax-3/
fn track_bracket_depth(token: &Token, closers: &mut Vec<Token>) -> bool {
    if Some(token) == closers.last() {
        closers.pop();
        closers.is_empty()
    } else {
        if let Some(closer) = matching_close(token) {
            closers.push(closer);
        }
        false
    }
}

/// Parses `tokens` fully with `production`, requiring every token to
/// be consumed — used to try `<media-condition>` and `<media-feature>`
/// in turn against an already-isolated `<media-in-parens>` block.
fn parse_fully<T>(
    tokens: &[PositionedToken],
    production: impl Fn(&mut Parser) -> Result<T, ParseError>,
) -> Result<T, ParseError> {
    let mut parser = Parser::new(tokens);
    let result = production(&mut parser)?;
    if parser.is_at_end() {
        Ok(result)
    } else {
        Err(ParseError::TrailingTokens(
            parser.peek().expect("not at end").clone(),
        ))
    }
}

fn expect_end(parser: &Parser) -> Result<(), ParseError> {
    match parser.peek() {
        None => Ok(()),
        Some(token) => Err(ParseError::TrailingTokens(token.clone())),
    }
}

/// Consumes an `<ident>` matching `keyword` case-insensitively, or
/// fails. Shared by `media_not`/`media_and`/`media_or` below, which
/// otherwise differ only in which keyword introduces their
/// `<media-in-parens>` operand.
fn consume_keyword(parser: &mut Parser, keyword: &'static str) -> Result<(), ParseError> {
    match parser.advance() {
        Some(Token::Ident(s)) if s.eq_ignore_ascii_case(keyword) => Ok(()),
        found => Err(ParseError::ExpectedKeyword { keyword, found }),
    }
}

/// `<media-not> = not <media-in-parens>`. Returns the operand, since
/// the `not` keyword itself carries no data.
fn media_not(parser: &mut Parser) -> Result<MediaInParens, ParseError> {
    consume_keyword(parser, "not")?;
    media_in_parens(parser)
}

/// `<media-and> = and <media-in-parens>`.
fn media_and(parser: &mut Parser) -> Result<MediaInParens, ParseError> {
    consume_keyword(parser, "and")?;
    media_in_parens(parser)
}

/// `<media-or> = or <media-in-parens>`.
fn media_or(parser: &mut Parser) -> Result<MediaInParens, ParseError> {
    consume_keyword(parser, "or")?;
    media_in_parens(parser)
}

/// Parses `<media-in-parens> [ keyword <media-in-parens> ]*`, given the
/// already-parsed leading `<media-in-parens>` as `first`, `keyword` as
/// the connective (`"and"`/`"or"`), and `next` as the per-iteration
/// production (`media_and`/`media_or`). Shared by `<media-condition>`'s
/// `and`- and `or`-branches and by `<media-condition-without-or>`,
/// which all reduce to exactly this "one operand, then zero or more
/// `keyword`-joined operands" shape.
fn in_parens_chain(
    parser: &mut Parser,
    first: MediaInParens,
    keyword: &'static str,
    next: impl Fn(&mut Parser) -> Result<MediaInParens, ParseError>,
) -> Result<Vec<MediaInParens>, ParseError> {
    let mut items = vec![first];
    while is_ident_ci(parser.peek(), keyword) {
        items.push(next(parser)?);
    }
    Ok(items)
}

/// Shared prefix of `<media-condition>` and `<media-condition-without-or>`:
/// both productions are `<media-not> | <media-in-parens> ...`. Returns
/// `(true, operand)` for the `<media-not>` branch (`operand` being its
/// `<media-in-parens>` operand), or `(false, operand)` for the plain
/// leading `<media-in-parens>`, for the caller to continue parsing.
fn media_not_or_first_in_parens(parser: &mut Parser) -> Result<(bool, MediaInParens), ParseError> {
    if is_ident_ci(parser.peek(), "not") {
        return Ok((true, media_not(parser)?));
    }
    Ok((false, media_in_parens(parser)?))
}

/// `<media-condition> = <media-not> | <media-in-parens> [ <media-and>* | <media-or>* ]`
fn media_condition(parser: &mut Parser) -> Result<MediaCondition, ParseError> {
    let (is_not, first) = media_not_or_first_in_parens(parser)?;
    if is_not {
        return Ok(MediaCondition::Not(first));
    }
    if is_ident_ci(parser.peek(), "or") {
        return Ok(MediaCondition::Or(in_parens_chain(
            parser, first, "or", media_or,
        )?));
    }
    Ok(MediaCondition::And(in_parens_chain(
        parser, first, "and", media_and,
    )?))
}

/// `<media-condition-without-or> = <media-not> | <media-in-parens> <media-and>*`
fn media_condition_without_or(parser: &mut Parser) -> Result<MediaConditionWithoutOr, ParseError> {
    let (is_not, first) = media_not_or_first_in_parens(parser)?;
    if is_not {
        return Ok(MediaConditionWithoutOr::Not(first));
    }
    Ok(MediaConditionWithoutOr::And(in_parens_chain(
        parser, first, "and", media_and,
    )?))
}

/// `<media-in-parens> = ( <media-condition> ) | ( <media-feature> ) | <general-enclosed>`
fn media_in_parens(parser: &mut Parser) -> Result<MediaInParens, ParseError> {
    match parser.peek() {
        Some(Token::OpenParen) => {
            let inner = parser.consume_block()?;
            if let Ok(condition) = parse_fully(&inner, media_condition) {
                return Ok(MediaInParens::Condition(Box::new(condition)));
            }
            if let Ok(feature) = parse_fully(&inner, media_feature) {
                return Ok(MediaInParens::Feature(feature));
            }
            Ok(MediaInParens::GeneralEnclosed(GeneralEnclosed {
                tokens: inner.into_iter().map(|(token, _)| token).collect(),
            }))
        }
        Some(Token::Function(_)) => {
            // <general-enclosed> alternative: [ <function-token> <any-value>? ) ]
            let function_token = parser.tokens[parser.pos].0.clone();
            let inner = parser.consume_block()?;
            let mut tokens = vec![function_token];
            tokens.extend(inner.into_iter().map(|(token, _)| token));
            Ok(MediaInParens::GeneralEnclosed(GeneralEnclosed { tokens }))
        }
        other => Err(ParseError::ExpectedMediaInParens(other.cloned())),
    }
}

/// `<mf-name> = <ident>`
fn mf_name(parser: &mut Parser) -> Result<MfName, ParseError> {
    match parser.advance() {
        Some(Token::Ident(name)) => Ok(MfName(name)),
        other => Err(ParseError::ExpectedMfName(other)),
    }
}

/// `<mf-value> = <number> | <dimension> | <ident> | <ratio>`
fn mf_value(parser: &mut Parser) -> Result<MfValue, ParseError> {
    match parser.advance() {
        Some(Token::Number { value, type_flag }) => {
            if matches!(parser.peek(), Some(Token::Delim('/'))) {
                parser.advance();
                match parser.advance() {
                    Some(Token::Number {
                        value: denominator,
                        type_flag: NumericType::Integer,
                    }) if type_flag == NumericType::Integer
                        && value >= 0.0
                        && denominator >= 0.0
                        && value <= u32::MAX as f64
                        && denominator <= u32::MAX as f64 =>
                    {
                        Ok(MfValue::Ratio {
                            numerator: value as u32,
                            denominator: denominator as u32,
                        })
                    }
                    _ => Err(ParseError::InvalidRatio),
                }
            } else {
                Ok(MfValue::Number(value))
            }
        }
        Some(Token::Dimension { value, unit, .. }) => Ok(MfValue::Dimension { value, unit }),
        Some(Token::Ident(name)) => Ok(MfValue::Ident(name)),
        other => Err(ParseError::ExpectedMfValue(other)),
    }
}

/// Whether `token` can start an `<mf-comparison>` (`<`, `<=`, `>`,
/// `>=`, or `=`) — i.e. is a `Delim` of `<`, `>`, or `=`.
fn starts_mf_comparison(token: Option<&Token>) -> bool {
    matches!(token, Some(Token::Delim('<' | '>' | '=')))
}

/// `<mf-comparison> = <mf-lt> | <mf-gt> | <mf-eq>`
/// `<mf-lt> = '<' '='?`, `<mf-gt> = '>' '='?`, `<mf-eq> = '='`.
///
/// The optional `=` in `<mf-lt>`/`<mf-gt>` must be *directly adjacent*
/// to the `<`/`>` — the grammar's `'<' '='?` denotes two adjacent
/// characters, not two independent component values separated by
/// whitespace — hence the [`Parser::current_preceded_by_whitespace`]
/// check before folding the `=` into `Le`/`Ge`. Without it, `<=` and
/// `< =` would be indistinguishable once whitespace tokens are
/// stripped (see [`PositionedToken`]).
fn mf_comparison(parser: &mut Parser) -> Result<MfComparison, ParseError> {
    match parser.advance() {
        Some(Token::Delim('<')) => {
            if parser.peek() == Some(&Token::Delim('=')) && !parser.current_preceded_by_whitespace()
            {
                parser.advance();
                Ok(MfComparison::Le)
            } else {
                Ok(MfComparison::Lt)
            }
        }
        Some(Token::Delim('>')) => {
            if parser.peek() == Some(&Token::Delim('=')) && !parser.current_preceded_by_whitespace()
            {
                parser.advance();
                Ok(MfComparison::Ge)
            } else {
                Ok(MfComparison::Gt)
            }
        }
        Some(Token::Delim('=')) => Ok(MfComparison::Eq),
        other => Err(ParseError::ExpectedMfComparison(other)),
    }
}

/// `<media-feature> = [ <mf-plain> | <mf-boolean> | <mf-range> ]`
///
/// Lookahead (see `plan/04-range-syntax.md` §"Parser-Änderungen"):
/// starting with `<mf-name>` (an `<ident>`) is ambiguous with starting
/// with `<mf-value>` when the value is itself an `<ident>`, so the
/// *following* token decides which grammar alternative applies:
///
/// 1. `<ident> :` → `<mf-plain>`.
/// 2. `<ident>` alone (end of block) → `<mf-boolean>`.
/// 3. `<ident>` followed by an `<mf-comparison>` start → `<mf-range>`,
///    `NameFirst` (`<mf-name> <mf-comparison> <mf-value>`).
/// 4. Anything else (a `<number>`/`<dimension>`, or an `<ident>` that
///    didn't match 1–3) → try `<mf-value> <mf-comparison> <mf-name>`
///    (`ValueFirst`), optionally continued by a second same-family
///    `<mf-lt>`/`<mf-gt>` and another `<mf-value>` (`Interval`).
fn media_feature(parser: &mut Parser) -> Result<MediaFeature, ParseError> {
    if matches!(parser.peek(), Some(Token::Ident(_))) {
        if matches!(parser.peek_at(1), Some(Token::Colon)) {
            let name = mf_name(parser)?;
            parser.advance();
            return Ok(MediaFeature::Plain {
                name,
                value: mf_value(parser)?,
            });
        }
        if parser.peek_at(1).is_none() {
            return Ok(MediaFeature::Boolean(mf_name(parser)?));
        }
        if starts_mf_comparison(parser.peek_at(1)) {
            let name = mf_name(parser)?;
            let operator = mf_comparison(parser)?;
            let value = mf_value(parser)?;
            return Ok(MediaFeature::Range(MfRange::NameFirst {
                name,
                operator,
                value,
            }));
        }
    }

    let value = mf_value(parser)?;
    let operator = mf_comparison(parser)?;
    let name = mf_name(parser)?;
    if parser.is_at_end() {
        return Ok(MediaFeature::Range(MfRange::ValueFirst {
            value,
            operator,
            name,
        }));
    }

    let (direction, lower_inclusive) = match operator {
        MfComparison::Lt => (MfRangeDirection::Ascending, false),
        MfComparison::Le => (MfRangeDirection::Ascending, true),
        MfComparison::Gt => (MfRangeDirection::Descending, false),
        MfComparison::Ge => (MfRangeDirection::Descending, true),
        MfComparison::Eq => return Err(ParseError::InvalidMfRangeInterval),
    };
    let upper_inclusive = match (direction, mf_comparison(parser)?) {
        (MfRangeDirection::Ascending, MfComparison::Lt) => false,
        (MfRangeDirection::Ascending, MfComparison::Le) => true,
        (MfRangeDirection::Descending, MfComparison::Gt) => false,
        (MfRangeDirection::Descending, MfComparison::Ge) => true,
        _ => return Err(ParseError::InvalidMfRangeInterval),
    };
    let upper = mf_value(parser)?;
    Ok(MediaFeature::Range(MfRange::Interval {
        lower: value,
        lower_inclusive,
        name,
        upper_inclusive,
        upper,
        direction,
    }))
}

/// `<media-type> = <ident>`, rejecting the keywords the grammar
/// excludes at this position (spec §3): `not`/`and`/`or`/`only`/`layer`.
fn media_type(parser: &mut Parser) -> Result<MediaType, ParseError> {
    match parser.advance() {
        Some(Token::Ident(name)) => {
            let lower = name.to_ascii_lowercase();
            if matches!(lower.as_str(), "not" | "and" | "or" | "only" | "layer") {
                Err(ParseError::ReservedMediaType(name))
            } else {
                Ok(MediaType(name))
            }
        }
        other => Err(ParseError::ExpectedMediaType(other)),
    }
}

/// The `[ not | only ]? <media-type> [ and <media-condition-without-or> ]?`
/// branch of `<media-query>`.
fn media_query_type_branch(parser: &mut Parser) -> Result<MediaQuery, ParseError> {
    let modifier = match parser.peek() {
        Some(Token::Ident(s)) if s.eq_ignore_ascii_case("not") => {
            parser.advance();
            Some(MediaModifier::Not)
        }
        Some(Token::Ident(s)) if s.eq_ignore_ascii_case("only") => {
            parser.advance();
            Some(MediaModifier::Only)
        }
        _ => None,
    };
    let media_type = media_type(parser)?;
    let condition = if is_ident_ci(parser.peek(), "and") {
        parser.advance();
        Some(media_condition_without_or(parser)?)
    } else {
        None
    };
    Ok(MediaQuery::TypeQuery {
        modifier,
        media_type,
        condition,
    })
}

/// `<media-query> = <media-condition>
///                | [ not | only ]? <media-type> [ and <media-condition-without-or> ]?`
///
/// The two branches are disambiguated by lookahead: a bare `(` or
/// function token starts `<media-condition>` directly; `not` followed
/// by `(`/a function token is the `<media-not>` branch of
/// `<media-condition>`, while `not` followed by anything else (an
/// `<ident>`) is the `not <media-type>` branch. `only` and a plain
/// `<ident>` always start the `<media-type>` branch.
fn media_query(parser: &mut Parser) -> Result<MediaQuery, ParseError> {
    let starts_condition = matches!(
        parser.peek(),
        Some(Token::OpenParen) | Some(Token::Function(_))
    ) || (is_ident_ci(parser.peek(), "not")
        && matches!(
            parser.peek_at(1),
            Some(Token::OpenParen) | Some(Token::Function(_))
        ));
    if starts_condition {
        Ok(MediaQuery::Condition(media_condition(parser)?))
    } else {
        media_query_type_branch(parser)
    }
}

/// Tokenizes `input`, strips whitespace/EOF tokens (which carry no
/// grammatical meaning above the tokenizer, see the module doc comment
/// on `Parser`), and pairs each remaining token with whether a
/// whitespace token immediately preceded it — see [`PositionedToken`].
fn prepare_tokens(input: &str) -> Vec<PositionedToken> {
    let mut result = Vec::new();
    let mut preceded_by_whitespace = false;
    for token in tokenize(input) {
        match token {
            Token::Whitespace => preceded_by_whitespace = true,
            Token::Eof => {}
            other => {
                result.push((other, preceded_by_whitespace));
                preceded_by_whitespace = false;
            }
        }
    }
    result
}

/// Splits `tokens` on top-level commas (per `<media-query-list> =
/// <media-query>#`), respecting bracket nesting so that a comma inside
/// a `<media-in-parens>` block doesn't split the list.
fn split_top_level_commas(tokens: &[PositionedToken]) -> Vec<Vec<PositionedToken>> {
    let mut segments = Vec::new();
    let mut current = Vec::new();
    let mut closers: Vec<Token> = Vec::new();
    for (token, preceded_by_whitespace) in tokens {
        if closers.is_empty() && *token == Token::Comma {
            segments.push(std::mem::take(&mut current));
            continue;
        }
        track_bracket_depth(token, &mut closers);
        current.push((token.clone(), *preceded_by_whitespace));
    }
    segments.push(current);
    segments
}

fn parse_media_query_tokens(tokens: &[PositionedToken]) -> Result<MediaQuery, ParseError> {
    if tokens.is_empty() {
        return Err(ParseError::EmptyMediaQuery);
    }
    let mut parser = Parser::new(tokens);
    let query = media_query(&mut parser)?;
    expect_end(&parser)?;
    Ok(query)
}

/// Parses `input` as a single `<media-query>`.
pub fn parse_media_query(input: &str) -> Result<MediaQuery, ParseError> {
    parse_media_query_tokens(&prepare_tokens(input))
}

/// Parses `input` as a `<media-query-list>`: a comma-separated list of
/// component values (spec §3), with each entry parsed independently as
/// a `<media-query>`. Returns one `Result` per entry rather than a
/// single `MediaQueryList`/error, since a single invalid entry must not
/// invalidate the rest of the list (see the module doc comment).
pub fn parse_media_query_list(input: &str) -> Vec<Result<MediaQuery, ParseError>> {
    let tokens = prepare_tokens(input);
    split_top_level_commas(&tokens)
        .into_iter()
        .map(|segment| parse_media_query_tokens(&segment))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::MediaCondition::*;
    use crate::ast::MediaInParens as InParens;

    fn feature_boolean(name: &str) -> InParens {
        InParens::Feature(MediaFeature::Boolean(MfName(name.into())))
    }

    fn feature_plain(name: &str, value: MfValue) -> InParens {
        InParens::Feature(MediaFeature::Plain {
            name: MfName(name.into()),
            value,
        })
    }

    fn feature_range(range: MfRange) -> InParens {
        InParens::Feature(MediaFeature::Range(range))
    }

    fn dim(value: f64, unit: &str) -> MfValue {
        MfValue::Dimension {
            value,
            unit: unit.into(),
        }
    }

    #[test]
    fn media_query_list_multiple_entries() {
        let results = parse_media_query_list("screen, print");
        assert_eq!(results.len(), 2);
        assert_eq!(
            results[0],
            Ok(MediaQuery::TypeQuery {
                modifier: None,
                media_type: MediaType("screen".into()),
                condition: None,
            })
        );
        assert_eq!(
            results[1],
            Ok(MediaQuery::TypeQuery {
                modifier: None,
                media_type: MediaType("print".into()),
                condition: None,
            })
        );
    }

    #[test]
    fn media_query_type_bare() {
        assert_eq!(
            parse_media_query("screen"),
            Ok(MediaQuery::TypeQuery {
                modifier: None,
                media_type: MediaType("screen".into()),
                condition: None,
            })
        );
    }

    #[test]
    fn media_query_type_with_not_modifier() {
        assert_eq!(
            parse_media_query("not screen"),
            Ok(MediaQuery::TypeQuery {
                modifier: Some(MediaModifier::Not),
                media_type: MediaType("screen".into()),
                condition: None,
            })
        );
    }

    #[test]
    fn media_query_type_with_only_modifier() {
        assert_eq!(
            parse_media_query("only screen"),
            Ok(MediaQuery::TypeQuery {
                modifier: Some(MediaModifier::Only),
                media_type: MediaType("screen".into()),
                condition: None,
            })
        );
    }

    #[test]
    fn media_query_type_with_and_condition() {
        assert_eq!(
            parse_media_query("screen and (color)"),
            Ok(MediaQuery::TypeQuery {
                modifier: None,
                media_type: MediaType("screen".into()),
                condition: Some(MediaConditionWithoutOr::And(vec![feature_boolean("color")])),
            })
        );
    }

    #[test]
    fn media_query_type_with_and_chain_condition() {
        assert_eq!(
            parse_media_query("screen and (color) and (monochrome)"),
            Ok(MediaQuery::TypeQuery {
                modifier: None,
                media_type: MediaType("screen".into()),
                condition: Some(MediaConditionWithoutOr::And(vec![
                    feature_boolean("color"),
                    feature_boolean("monochrome"),
                ])),
            })
        );
    }

    #[test]
    fn media_condition_without_or_rejects_or() {
        // "or" is not allowed in <media-condition-without-or>; it's
        // left over as a trailing token and must be a parse error.
        assert_eq!(
            parse_media_query("screen and (color) or (monochrome)"),
            Err(ParseError::TrailingTokens(Token::Ident("or".into())))
        );
    }

    #[test]
    fn media_query_condition_shorthand() {
        assert_eq!(
            parse_media_query("(color)"),
            Ok(MediaQuery::Condition(And(vec![feature_boolean("color")])))
        );
    }

    #[test]
    fn media_condition_not() {
        assert_eq!(
            parse_media_query("not (color)"),
            Ok(MediaQuery::Condition(Not(feature_boolean("color"))))
        );
    }

    #[test]
    fn media_condition_and_chain() {
        assert_eq!(
            parse_media_query("(color) and (monochrome)"),
            Ok(MediaQuery::Condition(And(vec![
                feature_boolean("color"),
                feature_boolean("monochrome"),
            ])))
        );
    }

    #[test]
    fn media_condition_or_chain() {
        assert_eq!(
            parse_media_query("(color) or (monochrome)"),
            Ok(MediaQuery::Condition(Or(vec![
                feature_boolean("color"),
                feature_boolean("monochrome"),
            ])))
        );
    }

    #[test]
    fn media_in_parens_condition() {
        assert_eq!(
            parse_media_query("((color) and (monochrome))"),
            Ok(MediaQuery::Condition(And(vec![InParens::Condition(
                Box::new(And(vec![
                    feature_boolean("color"),
                    feature_boolean("monochrome"),
                ]))
            )])))
        );
    }

    #[test]
    fn media_in_parens_feature() {
        assert_eq!(
            parse_media_query("(width: 400px)"),
            Ok(MediaQuery::Condition(And(vec![feature_plain(
                "width",
                MfValue::Dimension {
                    value: 400.0,
                    unit: "px".into()
                }
            )])))
        );
    }

    #[test]
    fn media_in_parens_general_enclosed() {
        let Ok(MediaQuery::Condition(And(items))) = parse_media_query("(3 + 5)") else {
            panic!("expected a bare condition with one <media-in-parens>");
        };
        assert_eq!(items.len(), 1);
        assert!(matches!(items[0], InParens::GeneralEnclosed(_)));
    }

    #[test]
    fn general_enclosed_function_token() {
        let Ok(MediaQuery::Condition(And(items))) = parse_media_query("foo(bar)") else {
            panic!("expected a bare condition with one <media-in-parens>");
        };
        assert_eq!(
            items,
            vec![InParens::GeneralEnclosed(GeneralEnclosed {
                tokens: vec![Token::Function("foo".into()), Token::Ident("bar".into())],
            })]
        );
    }

    #[test]
    fn mf_boolean() {
        assert_eq!(
            parse_media_query("(color)"),
            Ok(MediaQuery::Condition(And(vec![feature_boolean("color")])))
        );
    }

    #[test]
    fn mf_plain_number() {
        assert_eq!(
            parse_media_query("(color-index: 2)"),
            Ok(MediaQuery::Condition(And(vec![feature_plain(
                "color-index",
                MfValue::Number(2.0)
            )])))
        );
    }

    #[test]
    fn mf_plain_dimension() {
        assert_eq!(
            parse_media_query("(width: 400px)"),
            Ok(MediaQuery::Condition(And(vec![feature_plain(
                "width",
                MfValue::Dimension {
                    value: 400.0,
                    unit: "px".into()
                }
            )])))
        );
    }

    #[test]
    fn mf_plain_ident() {
        assert_eq!(
            parse_media_query("(orientation: landscape)"),
            Ok(MediaQuery::Condition(And(vec![feature_plain(
                "orientation",
                MfValue::Ident("landscape".into())
            )])))
        );
    }

    #[test]
    fn mf_plain_ratio() {
        assert_eq!(
            parse_media_query("(aspect-ratio: 16/9)"),
            Ok(MediaQuery::Condition(And(vec![feature_plain(
                "aspect-ratio",
                MfValue::Ratio {
                    numerator: 16,
                    denominator: 9
                }
            )])))
        );
    }

    #[test]
    fn mf_plain_ratio_out_of_u32_range_is_invalid() {
        let tokens = prepare_tokens("aspect-ratio: 99999999999999/1");
        assert_eq!(
            parse_fully(&tokens, media_feature),
            Err(ParseError::InvalidRatio)
        );
    }

    #[test]
    fn media_type_collision_rule() {
        assert_eq!(
            parse_media_query("layer"),
            Err(ParseError::ReservedMediaType("layer".into()))
        );
        assert_eq!(
            parse_media_query("and"),
            Err(ParseError::ReservedMediaType("and".into()))
        );
    }

    #[test]
    fn parse_error_on_unbalanced_parens() {
        assert_eq!(
            parse_media_query("(color"),
            Err(ParseError::UnbalancedParens)
        );
    }

    #[test]
    fn parse_media_query_list_surfaces_per_entry_errors() {
        let results = parse_media_query_list("screen, layer, (color");
        assert_eq!(results.len(), 3);
        assert!(results[0].is_ok());
        assert_eq!(
            results[1],
            Err(ParseError::ReservedMediaType("layer".into()))
        );
        assert_eq!(results[2], Err(ParseError::UnbalancedParens));
    }

    // --- `<mf-comparison>` (phase 04) ---

    #[test]
    fn mf_comparison_lt() {
        assert_eq!(
            parse_fully(&prepare_tokens("<"), mf_comparison),
            Ok(MfComparison::Lt)
        );
    }

    #[test]
    fn mf_comparison_gt() {
        assert_eq!(
            parse_fully(&prepare_tokens(">"), mf_comparison),
            Ok(MfComparison::Gt)
        );
    }

    #[test]
    fn mf_comparison_eq() {
        assert_eq!(
            parse_fully(&prepare_tokens("="), mf_comparison),
            Ok(MfComparison::Eq)
        );
    }

    #[test]
    fn mf_comparison_le_no_whitespace() {
        assert_eq!(
            parse_fully(&prepare_tokens("<="), mf_comparison),
            Ok(MfComparison::Le)
        );
    }

    #[test]
    fn mf_comparison_ge_no_whitespace() {
        assert_eq!(
            parse_fully(&prepare_tokens(">="), mf_comparison),
            Ok(MfComparison::Ge)
        );
    }

    #[test]
    fn mf_comparison_lt_then_eq_with_whitespace_is_not_le() {
        // Verifies the tokenizer/parser boundary from `plan/04-range-
        // syntax.md`: `<` and `=` tokenize as two independent `Delim`
        // tokens with no combined `<=` token (CSS Syntax Level 3 has
        // none). `mf_comparison` must only fold them into `Le` when
        // they're directly adjacent — with whitespace between, `<`
        // alone is a complete `<mf-lt>`, leaving the `=` as an
        // unconsumed trailing token.
        assert_eq!(
            parse_fully(&prepare_tokens("< ="), mf_comparison),
            Err(ParseError::TrailingTokens(Token::Delim('=')))
        );
    }

    #[test]
    fn mf_comparison_gt_then_eq_with_whitespace_is_not_ge() {
        assert_eq!(
            parse_fully(&prepare_tokens("> ="), mf_comparison),
            Err(ParseError::TrailingTokens(Token::Delim('=')))
        );
    }

    // --- `<mf-range>` (phase 04) ---

    #[test]
    fn mf_range_name_first_lt() {
        assert_eq!(
            parse_media_query("(width < 400px)"),
            Ok(MediaQuery::Condition(And(vec![feature_range(
                MfRange::NameFirst {
                    name: MfName("width".into()),
                    operator: MfComparison::Lt,
                    value: dim(400.0, "px"),
                }
            )])))
        );
    }

    #[test]
    fn mf_range_name_first_le() {
        assert_eq!(
            parse_media_query("(width <= 400px)"),
            Ok(MediaQuery::Condition(And(vec![feature_range(
                MfRange::NameFirst {
                    name: MfName("width".into()),
                    operator: MfComparison::Le,
                    value: dim(400.0, "px"),
                }
            )])))
        );
    }

    #[test]
    fn mf_range_name_first_gt() {
        assert_eq!(
            parse_media_query("(width > 400px)"),
            Ok(MediaQuery::Condition(And(vec![feature_range(
                MfRange::NameFirst {
                    name: MfName("width".into()),
                    operator: MfComparison::Gt,
                    value: dim(400.0, "px"),
                }
            )])))
        );
    }

    #[test]
    fn mf_range_name_first_ge() {
        assert_eq!(
            parse_media_query("(width >= 400px)"),
            Ok(MediaQuery::Condition(And(vec![feature_range(
                MfRange::NameFirst {
                    name: MfName("width".into()),
                    operator: MfComparison::Ge,
                    value: dim(400.0, "px"),
                }
            )])))
        );
    }

    #[test]
    fn mf_range_name_first_eq_with_ident_value() {
        assert_eq!(
            parse_media_query("(orientation = landscape)"),
            Ok(MediaQuery::Condition(And(vec![feature_range(
                MfRange::NameFirst {
                    name: MfName("orientation".into()),
                    operator: MfComparison::Eq,
                    value: MfValue::Ident("landscape".into()),
                }
            )])))
        );
    }

    #[test]
    fn mf_range_name_first_number_value() {
        assert_eq!(
            parse_media_query("(color-index >= 2)"),
            Ok(MediaQuery::Condition(And(vec![feature_range(
                MfRange::NameFirst {
                    name: MfName("color-index".into()),
                    operator: MfComparison::Ge,
                    value: MfValue::Number(2.0),
                }
            )])))
        );
    }

    #[test]
    fn mf_range_name_first_ratio_value() {
        assert_eq!(
            parse_media_query("(aspect-ratio >= 16/9)"),
            Ok(MediaQuery::Condition(And(vec![feature_range(
                MfRange::NameFirst {
                    name: MfName("aspect-ratio".into()),
                    operator: MfComparison::Ge,
                    value: MfValue::Ratio {
                        numerator: 16,
                        denominator: 9,
                    },
                }
            )])))
        );
    }

    #[test]
    fn mf_range_value_first_dimension() {
        assert_eq!(
            parse_media_query("(400px <= width)"),
            Ok(MediaQuery::Condition(And(vec![feature_range(
                MfRange::ValueFirst {
                    value: dim(400.0, "px"),
                    operator: MfComparison::Le,
                    name: MfName("width".into()),
                }
            )])))
        );
    }

    #[test]
    fn mf_range_value_first_number() {
        assert_eq!(
            parse_media_query("(2 < color-index)"),
            Ok(MediaQuery::Condition(And(vec![feature_range(
                MfRange::ValueFirst {
                    value: MfValue::Number(2.0),
                    operator: MfComparison::Lt,
                    name: MfName("color-index".into()),
                }
            )])))
        );
    }

    #[test]
    fn mf_range_value_first_ratio() {
        assert_eq!(
            parse_media_query("(16/9 <= aspect-ratio)"),
            Ok(MediaQuery::Condition(And(vec![feature_range(
                MfRange::ValueFirst {
                    value: MfValue::Ratio {
                        numerator: 16,
                        denominator: 9,
                    },
                    operator: MfComparison::Le,
                    name: MfName("aspect-ratio".into()),
                }
            )])))
        );
    }

    #[test]
    fn mf_range_interval_ascending_inclusive() {
        assert_eq!(
            parse_media_query("(400px <= width <= 700px)"),
            Ok(MediaQuery::Condition(And(vec![feature_range(
                MfRange::Interval {
                    lower: dim(400.0, "px"),
                    lower_inclusive: true,
                    name: MfName("width".into()),
                    upper_inclusive: true,
                    upper: dim(700.0, "px"),
                    direction: MfRangeDirection::Ascending,
                }
            )])))
        );
    }

    #[test]
    fn mf_range_interval_ascending_exclusive() {
        assert_eq!(
            parse_media_query("(400px < width < 700px)"),
            Ok(MediaQuery::Condition(And(vec![feature_range(
                MfRange::Interval {
                    lower: dim(400.0, "px"),
                    lower_inclusive: false,
                    name: MfName("width".into()),
                    upper_inclusive: false,
                    upper: dim(700.0, "px"),
                    direction: MfRangeDirection::Ascending,
                }
            )])))
        );
    }

    #[test]
    fn mf_range_interval_descending_inclusive() {
        assert_eq!(
            parse_media_query("(700px >= width >= 400px)"),
            Ok(MediaQuery::Condition(And(vec![feature_range(
                MfRange::Interval {
                    lower: dim(700.0, "px"),
                    lower_inclusive: true,
                    name: MfName("width".into()),
                    upper_inclusive: true,
                    upper: dim(400.0, "px"),
                    direction: MfRangeDirection::Descending,
                }
            )])))
        );
    }

    #[test]
    fn mf_range_interval_descending_exclusive() {
        assert_eq!(
            parse_media_query("(700px > width > 400px)"),
            Ok(MediaQuery::Condition(And(vec![feature_range(
                MfRange::Interval {
                    lower: dim(700.0, "px"),
                    lower_inclusive: false,
                    name: MfName("width".into()),
                    upper_inclusive: false,
                    upper: dim(400.0, "px"),
                    direction: MfRangeDirection::Descending,
                }
            )])))
        );
    }

    #[test]
    fn mf_range_interval_rejects_mixed_family() {
        let tokens = prepare_tokens("400px <= width > 700px");
        assert_eq!(
            parse_fully(&tokens, media_feature),
            Err(ParseError::InvalidMfRangeInterval)
        );
    }

    #[test]
    fn mf_range_interval_rejects_eq_as_first_operator() {
        let tokens = prepare_tokens("400px = width < 700px");
        assert_eq!(
            parse_fully(&tokens, media_feature),
            Err(ParseError::InvalidMfRangeInterval)
        );
    }

    #[test]
    fn mf_range_incomplete_name_first_is_invalid() {
        let tokens = prepare_tokens("width <");
        assert_eq!(
            parse_fully(&tokens, media_feature),
            Err(ParseError::ExpectedMfValue(None))
        );
    }

    #[test]
    fn mf_plain_and_boolean_regression_after_range_support() {
        // `<mf-plain>`/`<mf-boolean>` from phase 03 must keep working
        // unchanged now that `media_feature`'s lookahead also considers
        // `<mf-range>` — in particular an `<ident>` value (not a range
        // comparison) must still parse as `<mf-plain>`.
        assert_eq!(
            parse_media_query("(orientation: landscape)"),
            Ok(MediaQuery::Condition(And(vec![feature_plain(
                "orientation",
                MfValue::Ident("landscape".into())
            )])))
        );
        assert_eq!(
            parse_media_query("(color)"),
            Ok(MediaQuery::Condition(And(vec![feature_boolean("color")])))
        );
    }
}
