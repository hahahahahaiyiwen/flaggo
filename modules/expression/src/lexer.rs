use cel::common::ast::LiteralValue;
use cel::parser::pratt_parser::{Token, TokenKind as T};
use std::collections::HashMap;

pub(crate) struct Lexed {
    pub tokens: Vec<Token>,
    pub literals: HashMap<usize, LiteralValue>,
}

pub(crate) fn lex(source: &str) -> Result<Lexed, String> {
    let mut lexer = Lexer {
        source,
        cursor: 0,
        literals: HashMap::new(),
    };
    let mut tokens = Vec::new();
    while lexer.cursor < source.len() {
        let start = lexer.cursor;
        let rest = &source[start..];
        let first = rest.as_bytes()[0];
        if matches!(first, b' ' | b'\t' | b'\r' | b'\n' | b'\x0c') {
            lexer.cursor += 1;
            continue;
        }
        if rest.starts_with("//") {
            lexer.cursor += rest.find('\n').unwrap_or(rest.len());
            continue;
        }
        let kind = if first.is_ascii_digit()
            || (first == b'.' && rest.as_bytes().get(1).is_some_and(u8::is_ascii_digit))
        {
            lexer.number()?
        } else if let Some((prefix, raw, bytes)) = string_prefix(rest) {
            lexer.string(prefix, raw, bytes)?
        } else if first == b'`' {
            lexer.escaped_identifier()?;
            T::TokIdent
        } else if first.is_ascii_alphabetic() || first == b'_' {
            lexer.cursor += rest
                .bytes()
                .take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
                .count();
            match &source[start..lexer.cursor] {
                "true" => T::TokTrue,
                "false" => T::TokFalse,
                "null" => T::TokNull,
                "in" => T::TokIn,
                "as" | "break" | "const" | "continue" | "else" | "for" | "function" | "if"
                | "import" | "let" | "loop" | "package" | "namespace" | "return" | "var"
                | "void" | "while" => T::TokReservedWord,
                _ => T::TokIdent,
            }
        } else {
            lexer.cursor += 1;
            let paired = rest.as_bytes().get(1) == Some(&b'=');
            match first {
                b'(' => T::TokLeftParen,
                b')' => T::TokRightParen,
                b'[' => T::TokLeftBracket,
                b']' => T::TokRightBracket,
                b'{' => T::TokLeftBrace,
                b'}' => T::TokRightBrace,
                b'.' => T::TokDot,
                b',' => T::TokComma,
                b':' => T::TokColon,
                b'?' => T::TokQuestion,
                b'+' => T::TokPlus,
                b'-' => T::TokMinus,
                b'*' => T::TokAsterisk,
                b'/' => T::TokSlash,
                b'%' => T::TokPercent,
                b'!' | b'<' | b'>' | b'=' if paired => {
                    lexer.cursor += 1;
                    match first {
                        b'!' => T::TokExclamationEqual,
                        b'<' => T::TokLessEqual,
                        b'>' => T::TokGreaterEqual,
                        _ => T::TokEqualEqual,
                    }
                }
                b'!' => T::TokExclamation,
                b'<' => T::TokLess,
                b'>' => T::TokGreater,
                b'&' | b'|' if rest.as_bytes().get(1) == Some(&first) => {
                    lexer.cursor += 1;
                    if first == b'&' {
                        T::TokLogicalAnd
                    } else {
                        T::TokLogicalOr
                    }
                }
                b'=' | b'&' | b'|' => {
                    let character = char::from(first);
                    return Err(lexer.error(start, &format!(
                        "unexpected character '{character}' (did you mean '{character}{character}' ?)"
                    )));
                }
                _ => {
                    return Err(lexer.error(
                        start,
                        &format!(
                            "unexpected character '{}'",
                            diagnostic_character(rest.chars().next().unwrap())
                        ),
                    ));
                }
            }
        };
        tokens.push(Token {
            kind,
            start,
            end: lexer.cursor,
        });
    }
    tokens.push(Token {
        kind: T::TokEnd,
        start: source.len(),
        end: source.len(),
    });
    Ok(Lexed {
        tokens,
        literals: lexer.literals,
    })
}

fn string_prefix(source: &str) -> Option<(usize, bool, bool)> {
    let mut raw = false;
    let mut bytes = false;
    let mut prefix = 0;
    for byte in source.bytes().take(2) {
        match byte {
            b'r' | b'R' if !raw => raw = true,
            b'b' | b'B' if !bytes => bytes = true,
            _ => break,
        }
        prefix += 1;
    }
    matches!(source.as_bytes().get(prefix), Some(b'\'' | b'"')).then_some((prefix, raw, bytes))
}

fn diagnostic_character(character: char) -> char {
    // The reference quotes one UTF-16 unit; JSON replaces a lone high surrogate.
    if character.len_utf16() == 2 {
        char::REPLACEMENT_CHARACTER
    } else {
        character
    }
}

struct Lexer<'a> {
    source: &'a str,
    cursor: usize,
    literals: HashMap<usize, LiteralValue>,
}

impl Lexer<'_> {
    fn error(&self, offset: usize, message: &str) -> String {
        crate::syntax::diagnostic(self.source, offset, message)
    }

    fn number(&mut self) -> Result<T, String> {
        let start = self.cursor;
        let bytes = self.source.as_bytes();
        if bytes[start] == b'0' && matches!(bytes.get(start + 1), Some(b'x' | b'X')) {
            self.cursor += 2;
            let digits = self.cursor;
            while bytes.get(self.cursor).is_some_and(u8::is_ascii_hexdigit) {
                self.cursor += 1;
            }
            if self.cursor == digits {
                return Err(self.error(start, "invalid hex literal"));
            }
            if matches!(bytes.get(self.cursor), Some(b'u' | b'U')) {
                let value = u64::from_str_radix(&self.source[digits..self.cursor], 16);
                self.cursor += 1;
                return self.unsigned(start, value);
            }
            return Ok(T::TokInt);
        }
        let mut float = bytes[start] == b'.';
        if float {
            self.cursor += 1;
        }
        while bytes.get(self.cursor).is_some_and(u8::is_ascii_digit) {
            self.cursor += 1;
        }
        if !float
            && bytes.get(self.cursor) == Some(&b'.')
            && bytes.get(self.cursor + 1).is_some_and(u8::is_ascii_digit)
        {
            float = true;
            self.cursor += 1;
            while bytes.get(self.cursor).is_some_and(u8::is_ascii_digit) {
                self.cursor += 1;
            }
        }
        if matches!(bytes.get(self.cursor), Some(b'e' | b'E')) {
            let mut end = self.cursor + 1;
            if matches!(bytes.get(end), Some(b'+' | b'-')) {
                end += 1;
            }
            if bytes.get(end).is_some_and(u8::is_ascii_digit) {
                float = true;
                while bytes.get(end).is_some_and(u8::is_ascii_digit) {
                    end += 1;
                }
                self.cursor = end;
            }
        }
        if float {
            return Ok(T::TokFloat);
        }
        if matches!(bytes.get(self.cursor), Some(b'u' | b'U')) {
            let value = self.source[start..self.cursor].parse::<u64>();
            self.cursor += 1;
            return self.unsigned(start, value);
        }
        Ok(T::TokInt)
    }

    fn unsigned(
        &mut self,
        start: usize,
        value: Result<u64, std::num::ParseIntError>,
    ) -> Result<T, String> {
        let value = value.map_err(|_| {
            self.error(
                start,
                "invalid unsigned integer literal: value out of range",
            )
        })?;
        self.literals
            .insert(start, LiteralValue::UInt(value.into()));
        Ok(T::TokUint)
    }

    fn escaped_identifier(&mut self) -> Result<(), String> {
        let start = self.cursor;
        self.cursor += 1;
        while self.cursor < self.source.len() && self.source.as_bytes()[self.cursor] != b'`' {
            let character = self.source[self.cursor..].chars().next().unwrap();
            if !(character.is_ascii_alphanumeric()
                || matches!(character, '_' | '.' | '-' | '/' | ' '))
            {
                return Err(self.error(
                    self.cursor,
                    &format!(
                        "invalid character '{}' in escaped identifier",
                        diagnostic_character(character)
                    ),
                ));
            }
            self.cursor += 1;
        }
        if self.cursor == self.source.len() {
            return Err(self.error(start, "unterminated escaped identifier"));
        }
        if self.cursor == start + 1 {
            return Err(self.error(start, "empty escaped identifier"));
        }
        self.cursor += 1;
        Ok(())
    }

    fn string(&mut self, prefix: usize, raw: bool, bytes: bool) -> Result<T, String> {
        let start = self.cursor;
        self.cursor += prefix;
        let quote = self.source.as_bytes()[self.cursor];
        let width = if self.source.as_bytes()[self.cursor..].starts_with(&[quote; 3]) {
            3
        } else {
            1
        };
        self.cursor += width;
        let content_start = self.cursor;
        loop {
            if self.cursor == self.source.len() {
                return Err(self.error(start, "unterminated string literal"));
            }
            let rest = &self.source[self.cursor..];
            if rest.as_bytes().starts_with(&[quote; 3][..width]) {
                break;
            }
            let character = rest.chars().next().unwrap();
            if width == 1 && matches!(character, '\r' | '\n') {
                return Err(self.error(
                    start,
                    "unterminated string literal (newline in single-quoted string)",
                ));
            }
            self.cursor += character.len_utf8();
            if character == '\\' && !raw {
                let escaped = self.source[self.cursor..]
                    .chars()
                    .next()
                    .ok_or_else(|| self.error(start, "unterminated string literal"))?;
                self.cursor += escaped.len_utf8();
            }
        }
        let content = &self.source[content_start..self.cursor];
        let decoded = self.decode(content, content_start, raw, bytes)?;
        self.cursor += width;
        self.literals.insert(
            start,
            if bytes {
                LiteralValue::Bytes(decoded.into())
            } else {
                LiteralValue::String(
                    String::from_utf8(decoded)
                        .expect("decoded string is UTF-8")
                        .into(),
                )
            },
        );
        Ok(if bytes { T::TokBytes } else { T::TokString })
    }

    fn decode(
        &self,
        content: &str,
        offset: usize,
        raw: bool,
        bytes: bool,
    ) -> Result<Vec<u8>, String> {
        if raw {
            return Ok(content.as_bytes().to_vec());
        }
        let mut result = Vec::new();
        let mut cursor = 0;
        while cursor < content.len() {
            let character = content[cursor..].chars().next().unwrap();
            cursor += character.len_utf8();
            if character != '\\' {
                result.extend_from_slice(character.encode_utf8(&mut [0; 4]).as_bytes());
                continue;
            }
            let escape = content[cursor..]
                .chars()
                .next()
                .ok_or_else(|| self.error(offset + cursor, "invalid escape: trailing backslash"))?;
            let (value, consumed) = match escape {
                'a' => (7, 1),
                'b' => (8, 1),
                'f' => (12, 1),
                'n' => (10, 1),
                'r' => (13, 1),
                't' => (9, 1),
                'v' => (11, 1),
                '\\' | '?' | '"' | '\'' | '`' => (u32::from(escape), 1),
                'x' | 'X' => (self.hex(content, cursor + 1, 2, offset)?, 3),
                'u' | 'U' if !bytes => {
                    let digits = if escape == 'u' { 4 } else { 8 };
                    let value = self.hex(content, cursor + 1, digits, offset)?;
                    if (0xd800..=0xdfff).contains(&value) {
                        return Err(self.error(
                            offset + cursor,
                            "invalid unicode escape: surrogate code points are not allowed",
                        ));
                    }
                    if value > 0x10ffff {
                        return Err(self.error(
                            offset + cursor,
                            "invalid unicode escape: code point out of range",
                        ));
                    }
                    (value, digits + 1)
                }
                '0'..='3' => {
                    let digits = content
                        .as_bytes()
                        .get(cursor..cursor + 3)
                        .filter(|digits| {
                            digits[1..].iter().all(|digit| matches!(digit, b'0'..=b'7'))
                        })
                        .ok_or_else(|| {
                            self.error(
                                offset + cursor,
                                "invalid octal escape (expect 3 digits, [0-3][0-7][0-7])",
                            )
                        })?;
                    (
                        digits
                            .iter()
                            .fold(0, |value, digit| value * 8 + u32::from(digit - b'0')),
                        3,
                    )
                }
                _ => {
                    return Err(self.error(
                        offset + cursor,
                        &format!(
                            "invalid escape sequence '\\{}'",
                            diagnostic_character(escape)
                        ),
                    ));
                }
            };
            cursor += consumed;
            if bytes && matches!(escape, 'x' | 'X' | '0'..='3') {
                result.push(u8::try_from(value).expect("octet escape is at most 255"));
            } else {
                let character = char::from_u32(value).expect("escape is a Unicode scalar");
                result.extend_from_slice(character.encode_utf8(&mut [0; 4]).as_bytes());
            }
        }
        Ok(result)
    }

    fn hex(
        &self,
        content: &str,
        start: usize,
        digits: usize,
        offset: usize,
    ) -> Result<u32, String> {
        let remaining = &content[start..];
        // Reference escape widths count UTF-16 units, not UTF-8 bytes.
        if remaining.encode_utf16().count() < digits {
            return Err(self.error(offset + start, "invalid hex escape: too few digits"));
        }
        let mut value = 0;
        for (index, character) in remaining.char_indices().take(digits) {
            let digit = character
                .to_digit(16)
                .ok_or_else(|| self.error(offset + start + index, "invalid hex escape digit"))?;
            value = (value << 4) | digit;
        }
        Ok(value)
    }
}
