//! CSS tokenizer, implementing [CSS Syntax Module Level 3][spec] §3
//! (Tokenizing and Parsing CSS) and §4 (Tokenization).
//!
//! This module only turns a `&str` into a stream of tokens. It has no
//! knowledge of the Media Queries grammar (feature names, `and`/`or`/
//! `not`, range syntax, ...) — that belongs to the parser (phase 03).
//!
//! [spec]: https://www.w3.org/TR/css-syntax-3/

/// Numeric type flag, as used by [`Token::Number`] and
/// [`Token::Dimension`] (spec §4.3.12, "Consume a number").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum NumericType {
    /// The number had no fractional part or exponent.
    Integer,
    /// The number had a fractional part and/or an exponent.
    Number,
}

/// Type flag of a [`Token::Hash`] (spec §4.3.1, the `#` branch).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum HashType {
    /// The hash's value would be a valid ID selector (starts with an
    /// ident sequence).
    Id,
    /// The hash's value does not qualify as an ID selector.
    Unrestricted,
}

/// A single CSS token, per spec §4.3.1 ("Consume a token") and the
/// token types defined at the start of §4.3.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Token {
    /// An identifier, e.g. `screen`.
    Ident(String),
    /// An identifier immediately followed by `(`, e.g. `calc(`. The
    /// `(` itself is consumed but not part of the stored string.
    Function(String),
    /// `@` followed by an ident sequence, e.g. `@media`.
    AtKeyword(String),
    /// `#` followed by an ident-code-point/escape sequence, e.g. `#foo`.
    Hash {
        /// The value after `#`.
        value: String,
        /// Whether the value would itself be a valid ID selector.
        type_flag: HashType,
    },
    /// A quoted string, e.g. `"screen"`.
    String(String),
    /// A string that was not terminated correctly (e.g. an unescaped
    /// newline before the closing quote).
    BadString,
    /// A `url(...)` token with an unquoted value.
    Url(String),
    /// A `url(...)` construct that could not be tokenized as a valid
    /// URL token (e.g. an unescaped space in the unquoted value).
    BadUrl,
    /// A single code point that didn't start any other token, e.g. `^`.
    Delim(char),
    /// A numeric literal with no unit or `%` suffix, e.g. `42`.
    Number {
        /// The parsed numeric value.
        value: f64,
        /// Whether the literal had a fractional part/exponent.
        type_flag: NumericType,
    },
    /// A numeric literal followed by `%`, e.g. `50%`.
    Percentage {
        /// The parsed numeric value (before the `%`).
        value: f64,
    },
    /// A numeric literal followed by a unit, e.g. `10px`.
    Dimension {
        /// The parsed numeric value (before the unit).
        value: f64,
        /// Whether the literal had a fractional part/exponent.
        type_flag: NumericType,
        /// The unit identifier, e.g. `px`.
        unit: String,
    },
    /// One or more consecutive whitespace code points.
    Whitespace,
    /// `<!--`
    Cdo,
    /// `-->`
    Cdc,
    /// `:`
    Colon,
    /// `;`
    Semicolon,
    /// `,`
    Comma,
    /// `[`
    OpenSquare,
    /// `]`
    CloseSquare,
    /// `(`
    OpenParen,
    /// `)`
    CloseParen,
    /// `{`
    OpenCurly,
    /// `}`
    CloseCurly,
    /// The end of the input stream. Always the last token produced by
    /// [`Tokenizer`], never followed by another token.
    Eof,
}

/// Preprocesses the input stream per spec §3.3 ("Preprocessing the
/// input stream"):
///
/// - CR, FF, and CRLF are normalized to a single LF.
/// - U+0000 NULL is replaced with U+FFFD REPLACEMENT CHARACTER.
///
/// The spec also requires replacing lone surrogates with U+FFFD, but
/// that step is a no-op here: `input` is a Rust `&str`, which is
/// guaranteed to be valid UTF-8, and surrogate code points cannot
/// occur in valid UTF-8. There is no surrogate case to handle.
fn preprocess(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\u{000C}' => out.push('\n'),
            '\u{0000}' => out.push('\u{FFFD}'),
            other => out.push(other),
        }
    }
    out
}

/// Spec §4.2: "letter" is an uppercase or lowercase ASCII letter.
fn is_letter(c: char) -> bool {
    c.is_ascii_alphabetic()
}

/// Spec §4.2: "non-ASCII code point" is any code point >= U+0080.
fn is_non_ascii(c: char) -> bool {
    !c.is_ascii()
}

/// Spec §4.2: "ident-start code point".
fn is_ident_start(c: char) -> bool {
    is_letter(c) || is_non_ascii(c) || c == '_'
}

/// Spec §4.2: "ident code point".
fn is_ident_code_point(c: char) -> bool {
    is_ident_start(c) || c.is_ascii_digit() || c == '-'
}

/// Spec §4.2: "non-printable code point".
fn is_non_printable(c: char) -> bool {
    matches!(c, '\u{0000}'..='\u{0008}' | '\u{000B}' | '\u{000E}'..='\u{001F}' | '\u{007F}')
}

/// Spec §4.2: "whitespace" (newline, tab, or space; CR/FF have already
/// been normalized away by [`preprocess`]).
fn is_whitespace(c: char) -> bool {
    matches!(c, '\n' | '\t' | ' ')
}

fn is_surrogate(value: u32) -> bool {
    (0xD800..=0xDFFF).contains(&value)
}

/// A tokenizer over a preprocessed input stream, per spec §4.3.
///
/// Implements [`Iterator`], yielding tokens in order and ending with a
/// single [`Token::Eof`], after which it yields `None`. See
/// `plan/DECISIONS.md` for why this shape (iterator + terminal EOF
/// token, plus the [`tokenize`] convenience function) was chosen over
/// alternatives.
pub struct Tokenizer {
    input: Vec<char>,
    pos: usize,
    done: bool,
}

impl Tokenizer {
    /// Creates a tokenizer over `input`, applying the spec §3.3
    /// preprocessing step (newline normalization, NULL replacement) up
    /// front.
    pub fn new(input: &str) -> Self {
        Self {
            input: preprocess(input).chars().collect(),
            pos: 0,
            done: false,
        }
    }

    fn peek_n(&self, n: usize) -> Option<char> {
        self.input.get(self.pos + n).copied()
    }

    fn peek(&self) -> Option<char> {
        self.peek_n(0)
    }

    /// Consumes as much whitespace as possible. Shared substep of several
    /// spec algorithms (not itself a named spec algorithm).
    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(w) if is_whitespace(w)) {
            self.pos += 1;
        }
    }

    /// Spec §4.3.8: "Check if two code points are a valid escape",
    /// applied to the two code points starting at `offset`.
    fn is_valid_escape_at(&self, offset: usize) -> bool {
        match self.peek_n(offset) {
            Some('\\') => !matches!(self.peek_n(offset + 1), Some('\n')),
            _ => false,
        }
    }

    /// Spec §4.3.9: "Check if three code points would start an ident
    /// sequence", applied starting at `offset`.
    fn starts_ident_sequence(&self, offset: usize) -> bool {
        match self.peek_n(offset) {
            Some('-') => match self.peek_n(offset + 1) {
                Some(c) if is_ident_start(c) || c == '-' => true,
                _ => self.is_valid_escape_at(offset + 1),
            },
            Some(c) if is_ident_start(c) => true,
            Some('\\') => self.is_valid_escape_at(offset),
            _ => false,
        }
    }

    /// Spec §4.3.10: "Check if three code points would start a
    /// number", applied starting at `offset`.
    fn starts_number(&self, offset: usize) -> bool {
        match self.peek_n(offset) {
            Some('+') | Some('-') => match self.peek_n(offset + 1) {
                Some(c) if c.is_ascii_digit() => true,
                Some('.') => matches!(self.peek_n(offset + 2), Some(c) if c.is_ascii_digit()),
                _ => false,
            },
            Some('.') => matches!(self.peek_n(offset + 1), Some(c) if c.is_ascii_digit()),
            Some(c) if c.is_ascii_digit() => true,
            _ => false,
        }
    }

    /// Spec §4.3.2: "Consume comments". Comments produce no token.
    fn consume_comments(&mut self) {
        while self.peek() == Some('/') && self.peek_n(1) == Some('*') {
            self.pos += 2;
            loop {
                match self.peek() {
                    None => return, // parse error: EOF inside comment
                    Some('*') if self.peek_n(1) == Some('/') => {
                        self.pos += 2;
                        break;
                    }
                    _ => self.pos += 1,
                }
            }
        }
    }

    /// Spec §4.3.7: "Consume an escaped code point". Assumes the
    /// leading backslash has already been consumed.
    fn consume_escaped_code_point(&mut self) -> char {
        match self.peek() {
            Some(c) if c.is_ascii_hexdigit() => {
                let mut hex = String::new();
                hex.push(c);
                self.pos += 1;
                for _ in 0..5 {
                    match self.peek() {
                        Some(h) if h.is_ascii_hexdigit() => {
                            hex.push(h);
                            self.pos += 1;
                        }
                        _ => break,
                    }
                }
                if matches!(self.peek(), Some(w) if is_whitespace(w)) {
                    self.pos += 1;
                }
                let value = u32::from_str_radix(&hex, 16).unwrap_or(0);
                if value == 0 || is_surrogate(value) || value > 0x10FFFF {
                    '\u{FFFD}'
                } else {
                    char::from_u32(value).unwrap_or('\u{FFFD}')
                }
            }
            Some(c) => {
                self.pos += 1;
                c
            }
            None => '\u{FFFD}', // parse error: EOF
        }
    }

    /// Spec §4.3.11: "Consume an ident sequence".
    fn consume_ident_sequence(&mut self) -> String {
        let mut result = String::new();
        loop {
            match self.peek() {
                Some(c) if is_ident_code_point(c) => {
                    result.push(c);
                    self.pos += 1;
                }
                Some('\\') if self.is_valid_escape_at(0) => {
                    self.pos += 1;
                    let ch = self.consume_escaped_code_point();
                    result.push(ch);
                }
                _ => break,
            }
        }
        result
    }

    /// Spec §4.3.12: "Consume a number". Returns the (grammar-exact)
    /// representation string together with its type flag.
    ///
    /// The representation is parsed with `str::parse::<f64>`, which
    /// implements the same sign/integer/fraction/exponent semantics
    /// as spec §4.3.13 ("Convert a string to a number") for any
    /// string this algorithm can produce.
    fn consume_number(&mut self) -> (f64, NumericType) {
        let mut repr = String::new();
        let mut type_flag = NumericType::Integer;

        if matches!(self.peek(), Some('+') | Some('-')) {
            repr.push(self.peek().unwrap());
            self.pos += 1;
        }
        while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
            repr.push(self.peek().unwrap());
            self.pos += 1;
        }
        if self.peek() == Some('.') && matches!(self.peek_n(1), Some(c) if c.is_ascii_digit()) {
            repr.push('.');
            self.pos += 1;
            type_flag = NumericType::Number;
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                repr.push(self.peek().unwrap());
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some('e') | Some('E')) {
            let has_sign = matches!(self.peek_n(1), Some('+') | Some('-'));
            let digit_offset = if has_sign { 2 } else { 1 };
            if matches!(self.peek_n(digit_offset), Some(c) if c.is_ascii_digit()) {
                repr.push(self.peek().unwrap());
                self.pos += 1;
                if has_sign {
                    repr.push(self.peek().unwrap());
                    self.pos += 1;
                }
                type_flag = NumericType::Number;
                while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                    repr.push(self.peek().unwrap());
                    self.pos += 1;
                }
            }
        }

        // `repr` is only reached via `starts_number`, which guarantees
        // a syntactically valid CSS number literal, so this always
        // parses.
        let value = repr
            .parse::<f64>()
            .expect("consume_number produced a valid number literal");
        (value, type_flag)
    }

    /// Spec §4.3.3: "Consume a numeric token".
    fn consume_numeric_token(&mut self) -> Token {
        let (value, type_flag) = self.consume_number();
        if self.starts_ident_sequence(0) {
            let unit = self.consume_ident_sequence();
            Token::Dimension {
                value,
                type_flag,
                unit,
            }
        } else if self.peek() == Some('%') {
            self.pos += 1;
            Token::Percentage { value }
        } else {
            Token::Number { value, type_flag }
        }
    }

    /// Spec §4.3.14: "Consume the remnants of a bad url", used for
    /// error recovery after a bad-url-token.
    fn consume_bad_url_remnants(&mut self) {
        loop {
            match self.peek() {
                Some(')') => {
                    self.pos += 1;
                    return;
                }
                None => return,
                _ if self.is_valid_escape_at(0) => {
                    self.pos += 1;
                    self.consume_escaped_code_point();
                }
                _ => self.pos += 1,
            }
        }
    }

    /// Spec §4.3.6: "Consume a url token". Assumes the leading `url(`
    /// has already been consumed.
    fn consume_url_token(&mut self) -> Token {
        let mut value = String::new();
        self.skip_whitespace();
        loop {
            match self.peek() {
                None => return Token::Url(value), // parse error: EOF
                Some(')') => {
                    self.pos += 1;
                    return Token::Url(value);
                }
                Some(c) if is_whitespace(c) => {
                    self.skip_whitespace();
                    match self.peek() {
                        Some(')') => {
                            self.pos += 1;
                            return Token::Url(value);
                        }
                        None => return Token::Url(value), // parse error: EOF
                        _ => {
                            self.consume_bad_url_remnants();
                            return Token::BadUrl;
                        }
                    }
                }
                Some('"') | Some('\'') | Some('(') => {
                    self.pos += 1; // parse error
                    self.consume_bad_url_remnants();
                    return Token::BadUrl;
                }
                Some(c) if is_non_printable(c) => {
                    self.pos += 1; // parse error
                    self.consume_bad_url_remnants();
                    return Token::BadUrl;
                }
                Some('\\') => {
                    if self.is_valid_escape_at(0) {
                        self.pos += 1;
                        let ch = self.consume_escaped_code_point();
                        value.push(ch);
                    } else {
                        self.pos += 1; // parse error
                        self.consume_bad_url_remnants();
                        return Token::BadUrl;
                    }
                }
                Some(c) => {
                    value.push(c);
                    self.pos += 1;
                }
            }
        }
    }

    /// Spec §4.3.4: "Consume an ident-like token".
    fn consume_ident_like_token(&mut self) -> Token {
        let s = self.consume_ident_sequence();
        if s.eq_ignore_ascii_case("url") && self.peek() == Some('(') {
            self.pos += 1;
            while matches!(self.peek(), Some(a) if is_whitespace(a))
                && matches!(self.peek_n(1), Some(b) if is_whitespace(b))
            {
                self.pos += 1;
            }
            let starts_quoted = matches!(self.peek(), Some('"') | Some('\''))
                || (matches!(self.peek(), Some(a) if is_whitespace(a))
                    && matches!(self.peek_n(1), Some('"') | Some('\'')));
            if starts_quoted {
                Token::Function(s)
            } else {
                self.consume_url_token()
            }
        } else if self.peek() == Some('(') {
            self.pos += 1;
            Token::Function(s)
        } else {
            Token::Ident(s)
        }
    }

    /// Spec §4.3.5: "Consume a string token", with `ending` as the
    /// ending code point.
    fn consume_string_token(&mut self, ending: char) -> Token {
        let mut value = String::new();
        loop {
            match self.peek() {
                Some(c) if c == ending => {
                    self.pos += 1;
                    return Token::String(value);
                }
                None => return Token::String(value), // parse error: EOF
                Some('\n') => return Token::BadString, // parse error; reconsume the newline
                Some('\\') => {
                    self.pos += 1;
                    match self.peek() {
                        None => {}
                        Some('\n') => self.pos += 1,
                        Some(_) => {
                            let ch = self.consume_escaped_code_point();
                            value.push(ch);
                        }
                    }
                }
                Some(c) => {
                    value.push(c);
                    self.pos += 1;
                }
            }
        }
    }

    /// Spec §4.3.1: "Consume a token".
    fn consume_token(&mut self) -> Token {
        self.consume_comments();
        match self.peek() {
            None => Token::Eof,
            Some(c) if is_whitespace(c) => {
                self.skip_whitespace();
                Token::Whitespace
            }
            Some('"') => {
                self.pos += 1;
                self.consume_string_token('"')
            }
            Some('#') => {
                self.pos += 1;
                if matches!(self.peek(), Some(c) if is_ident_code_point(c))
                    || self.is_valid_escape_at(0)
                {
                    let type_flag = if self.starts_ident_sequence(0) {
                        HashType::Id
                    } else {
                        HashType::Unrestricted
                    };
                    let value = self.consume_ident_sequence();
                    Token::Hash { value, type_flag }
                } else {
                    Token::Delim('#')
                }
            }
            Some('\'') => {
                self.pos += 1;
                self.consume_string_token('\'')
            }
            Some('(') => {
                self.pos += 1;
                Token::OpenParen
            }
            Some(')') => {
                self.pos += 1;
                Token::CloseParen
            }
            Some('+') => {
                if self.starts_number(0) {
                    self.consume_numeric_token()
                } else {
                    self.pos += 1;
                    Token::Delim('+')
                }
            }
            Some(',') => {
                self.pos += 1;
                Token::Comma
            }
            Some('-') => {
                if self.starts_number(0) {
                    self.consume_numeric_token()
                } else if self.peek_n(1) == Some('-') && self.peek_n(2) == Some('>') {
                    self.pos += 3;
                    Token::Cdc
                } else if self.starts_ident_sequence(0) {
                    self.consume_ident_like_token()
                } else {
                    self.pos += 1;
                    Token::Delim('-')
                }
            }
            Some('.') => {
                if self.starts_number(0) {
                    self.consume_numeric_token()
                } else {
                    self.pos += 1;
                    Token::Delim('.')
                }
            }
            Some(':') => {
                self.pos += 1;
                Token::Colon
            }
            Some(';') => {
                self.pos += 1;
                Token::Semicolon
            }
            Some('<') => {
                if self.peek_n(1) == Some('!')
                    && self.peek_n(2) == Some('-')
                    && self.peek_n(3) == Some('-')
                {
                    self.pos += 4;
                    Token::Cdo
                } else {
                    self.pos += 1;
                    Token::Delim('<')
                }
            }
            Some('@') => {
                self.pos += 1;
                if self.starts_ident_sequence(0) {
                    let value = self.consume_ident_sequence();
                    Token::AtKeyword(value)
                } else {
                    Token::Delim('@')
                }
            }
            Some('[') => {
                self.pos += 1;
                Token::OpenSquare
            }
            Some('\\') => {
                if self.is_valid_escape_at(0) {
                    self.consume_ident_like_token()
                } else {
                    self.pos += 1; // parse error
                    Token::Delim('\\')
                }
            }
            Some(']') => {
                self.pos += 1;
                Token::CloseSquare
            }
            Some('{') => {
                self.pos += 1;
                Token::OpenCurly
            }
            Some('}') => {
                self.pos += 1;
                Token::CloseCurly
            }
            Some(c) if c.is_ascii_digit() => self.consume_numeric_token(),
            Some(c) if is_ident_start(c) => self.consume_ident_like_token(),
            Some(c) => {
                self.pos += 1;
                Token::Delim(c)
            }
        }
    }
}

impl Iterator for Tokenizer {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        if self.done {
            return None;
        }
        let token = self.consume_token();
        if token == Token::Eof {
            self.done = true;
        }
        Some(token)
    }
}

/// Tokenizes `input` into a `Vec<Token>`, ending with a single
/// [`Token::Eof`]. Convenience wrapper around [`Tokenizer`] for
/// callers that don't need streaming/lazy tokenization.
pub fn tokenize(input: &str) -> Vec<Token> {
    Tokenizer::new(input).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(input: &str) -> Vec<Token> {
        tokenize(input)
    }

    #[test]
    fn ident_token() {
        assert_eq!(tokens("foo"), vec![Token::Ident("foo".into()), Token::Eof]);
    }

    #[test]
    fn ident_token_with_escape() {
        assert_eq!(
            tokens(r"\41 bc"),
            vec![Token::Ident("Abc".into()), Token::Eof]
        );
    }

    #[test]
    fn function_token() {
        assert_eq!(
            tokens("foo("),
            vec![Token::Function("foo".into()), Token::Eof]
        );
    }

    #[test]
    fn at_keyword_token() {
        assert_eq!(
            tokens("@media"),
            vec![Token::AtKeyword("media".into()), Token::Eof]
        );
    }

    #[test]
    fn hash_token_id() {
        assert_eq!(
            tokens("#foo"),
            vec![
                Token::Hash {
                    value: "foo".into(),
                    type_flag: HashType::Id
                },
                Token::Eof
            ]
        );
    }

    #[test]
    fn hash_token_unrestricted() {
        assert_eq!(
            tokens("#1"),
            vec![
                Token::Hash {
                    value: "1".into(),
                    type_flag: HashType::Unrestricted
                },
                Token::Eof
            ]
        );
    }

    #[test]
    fn string_token() {
        assert_eq!(
            tokens("\"hello\""),
            vec![Token::String("hello".into()), Token::Eof]
        );
        assert_eq!(
            tokens("'hello'"),
            vec![Token::String("hello".into()), Token::Eof]
        );
    }

    #[test]
    fn bad_string_token_on_unterminated_newline() {
        assert_eq!(
            tokens("\"abc\ndef\""),
            vec![
                Token::BadString,
                Token::Whitespace,
                Token::Ident("def".into()),
                Token::String("".into()),
                Token::Eof,
            ]
        );
    }

    #[test]
    fn url_token() {
        assert_eq!(
            tokens("url(foo.png)"),
            vec![Token::Url("foo.png".into()), Token::Eof]
        );
    }

    #[test]
    fn url_token_quoted_is_a_function() {
        assert_eq!(
            tokens("url(\"foo.png\")"),
            vec![
                Token::Function("url".into()),
                Token::String("foo.png".into()),
                Token::CloseParen,
                Token::Eof
            ]
        );
    }

    #[test]
    fn bad_url_token_on_unescaped_space() {
        assert_eq!(tokens("url(foo bar)"), vec![Token::BadUrl, Token::Eof]);
    }

    #[test]
    fn delim_token() {
        assert_eq!(tokens("^"), vec![Token::Delim('^'), Token::Eof]);
    }

    #[test]
    fn number_token_integer() {
        assert_eq!(
            tokens("42"),
            vec![
                Token::Number {
                    value: 42.0,
                    type_flag: NumericType::Integer
                },
                Token::Eof
            ]
        );
    }

    #[test]
    fn number_token_fractional() {
        assert_eq!(
            tokens("4.2"),
            vec![
                Token::Number {
                    value: 4.2,
                    type_flag: NumericType::Number
                },
                Token::Eof
            ]
        );
    }

    #[test]
    fn percentage_token() {
        assert_eq!(
            tokens("50%"),
            vec![Token::Percentage { value: 50.0 }, Token::Eof]
        );
    }

    #[test]
    fn dimension_token() {
        assert_eq!(
            tokens("10px"),
            vec![
                Token::Dimension {
                    value: 10.0,
                    type_flag: NumericType::Integer,
                    unit: "px".into()
                },
                Token::Eof
            ]
        );
    }

    #[test]
    fn whitespace_token() {
        assert_eq!(tokens("  \t\n"), vec![Token::Whitespace, Token::Eof]);
    }

    #[test]
    fn cdo_token() {
        assert_eq!(tokens("<!--"), vec![Token::Cdo, Token::Eof]);
    }

    #[test]
    fn cdc_token() {
        assert_eq!(tokens("-->"), vec![Token::Cdc, Token::Eof]);
    }

    #[test]
    fn colon_semicolon_comma_tokens() {
        assert_eq!(
            tokens(":;,"),
            vec![Token::Colon, Token::Semicolon, Token::Comma, Token::Eof]
        );
    }

    #[test]
    fn bracket_tokens() {
        assert_eq!(
            tokens("[](){}"),
            vec![
                Token::OpenSquare,
                Token::CloseSquare,
                Token::OpenParen,
                Token::CloseParen,
                Token::OpenCurly,
                Token::CloseCurly,
                Token::Eof,
            ]
        );
    }

    #[test]
    fn eof_token_on_empty_input() {
        assert_eq!(tokens(""), vec![Token::Eof]);
    }

    #[test]
    fn comments_are_transparent() {
        assert_eq!(
            tokens("/* comment */foo"),
            vec![Token::Ident("foo".into()), Token::Eof]
        );
    }

    #[test]
    fn preprocessing_normalizes_newlines() {
        assert_eq!(tokens("a\r\nb"), tokens("a\nb"));
        assert_eq!(tokens("a\rb"), tokens("a\nb"));
        assert_eq!(tokens("a\u{000C}b"), tokens("a\nb"));
    }

    #[test]
    fn preprocessing_replaces_null_with_replacement_character() {
        assert_eq!(
            tokens("\u{0000}"),
            vec![Token::Ident("\u{FFFD}".into()), Token::Eof]
        );
    }
}
