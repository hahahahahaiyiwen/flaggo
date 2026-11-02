use cel::common::ast::{
    CallExpr, EntryExpr, Expr, IdedEntryExpr, IdedExpr, ListExpr, LiteralValue, MapEntryExpr,
    MapExpr, SelectExpr, StructExpr, StructFieldExpr,
};
use cel::parser::pratt_parser::{Token, TokenKind as T};
use std::collections::HashMap;

pub(crate) struct Parsed<'a> {
    pub expression: IdedExpr,
    source: &'a str,
    positions: HashMap<u64, usize>,
}

impl Parsed<'_> {
    pub fn diagnostic(&self, node: &IdedExpr, message: &str) -> String {
        diagnostic(self.source, self.positions[&node.id], message)
    }
}

pub(crate) fn diagnostic(source: &str, offset: usize, message: &str) -> String {
    let prefix = &source[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit('\n')
        .next()
        .unwrap_or("")
        .encode_utf16()
        .count()
        + 1;
    format!("ERROR: {line}:{column}: {message}")
}

pub(crate) fn parse(source: &str) -> Result<Parsed<'_>, String> {
    if source.trim().is_empty() {
        return Err("Expression must not be empty.".to_owned());
    }
    if source.len() > crate::MAXIMUM_EXPRESSION_BYTES {
        return Err("Expression exceeds the 4096-byte limit.".to_owned());
    }
    let lexed = crate::lexer::lex(source)?;
    let mut parser = Parser {
        source,
        tokens: lexed.tokens,
        literals: lexed.literals,
        cursor: 0,
        depth: 0,
        positions: HashMap::new(),
    };
    let expression = *parser.expression()?;
    if parser.current().kind != T::TokEnd {
        return Err(parser.error(&format!(
            "unexpected token {} after expression",
            parser.describe(),
        )));
    }
    let mut pending = vec![(&expression, 1)];
    let mut maximum = 0;
    while let Some((node, depth)) = pending.pop() {
        maximum = maximum.max(depth);
        match &node.expr {
            Expr::Call(call) => {
                pending.extend(call.args.iter().map(|node| (node, depth + 1)));
                if let Some(target) = &call.target {
                    pending.push((target, depth + 1));
                }
            }
            Expr::Select(select) => pending.push((&select.operand, depth + 1)),
            Expr::List(list) => pending.extend(list.elements.iter().map(|node| (node, depth + 1))),
            Expr::Map(map) => {
                for entry in &map.entries {
                    if let EntryExpr::MapEntry(entry) = &entry.expr {
                        pending.push((&entry.key, depth + 1));
                        pending.push((&entry.value, depth + 1));
                    }
                }
            }
            Expr::Struct(value) => {
                for entry in &value.entries {
                    if let EntryExpr::StructField(entry) = &entry.expr {
                        pending.push((&entry.value, depth + 1));
                    }
                }
            }
            _ => {}
        }
    }
    if maximum > 1000 {
        return Err(diagnostic(
            source,
            0,
            &format!("expression depth limit exceeded: {maximum} > 1000"),
        ));
    }
    Ok(Parsed {
        expression,
        source,
        positions: parser.positions,
    })
}

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    literals: HashMap<usize, LiteralValue>,
    cursor: usize,
    depth: usize,
    positions: HashMap<u64, usize>,
}

struct Collection {
    open: Token,
    structure: Option<String>,
    elements: Vec<IdedExpr>,
    entries: Vec<IdedEntryExpr>,
}

impl Collection {
    fn is_list(&self) -> bool {
        self.open.kind == T::TokLeftBracket
    }

    fn close(&self) -> T {
        if self.is_list() {
            T::TokRightBracket
        } else {
            T::TokRightBrace
        }
    }
}

struct Call {
    offset: usize,
    name: String,
    target: Option<Box<IdedExpr>>,
    args: Vec<IdedExpr>,
}

enum Frame {
    Expression,
    EndExpression,
    Conditional,
    Truthy(Box<IdedExpr>, usize),
    Falsy(Box<IdedExpr>, Box<IdedExpr>, usize),
    Binary(u8),
    BinaryTail(u8),
    BinaryRight(u8, Box<IdedExpr>, usize, &'static str),
    Unary,
    ApplyUnary(usize, &'static str),
    Primary,
    Suffix,
    CloseParen,
    Index(Box<IdedExpr>, usize),
    Arguments(Call),
    Argument(Call),
    Collection(Collection),
    CollectionTail(Collection),
    Element(Collection),
    MapKey(Collection, usize),
    MapValue(Collection, usize, Box<IdedExpr>),
    StructValue(Collection, usize, String),
}

// Boxed operands keep the heap-backed continuation stack compact.
#[allow(clippy::vec_box)]
impl Parser<'_> {
    fn current(&self) -> Token {
        self.tokens[self.cursor]
    }

    fn text(&self, token: Token) -> &str {
        &self.source[token.start..token.end]
    }

    fn advance(&mut self) -> Token {
        let token = self.current();
        self.cursor += 1;
        token
    }

    fn take(&mut self, kind: T) -> bool {
        if self.current().kind == kind {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: T, name: &str) -> Result<Token, String> {
        if self.current().kind != kind {
            return Err(self.error(&format!("expected {name}, found {}", self.describe())));
        }
        Ok(self.advance())
    }

    fn describe(&self) -> String {
        match self.current().kind {
            T::TokEnd => "end of expression".into(),
            T::TokString => "string literal".into(),
            T::TokBytes => "bytes literal".into(),
            T::TokInt | T::TokUint | T::TokFloat => "numeric literal".into(),
            T::TokIdent if self.text(self.current()).starts_with('`') => "'EscapedIdent'".into(),
            T::TokIdent | T::TokReservedWord => format!("'{}'", self.text(self.current())),
            kind => format!("'{}'", kind.as_str()),
        }
    }

    fn error(&self, message: &str) -> String {
        diagnostic(self.source, self.current().start, message)
    }

    fn id(&mut self, offset: usize) -> u64 {
        let id = self.positions.len() as u64 + 1;
        self.positions.insert(id, offset);
        id
    }

    fn node(&mut self, offset: usize, expr: Expr) -> Box<IdedExpr> {
        Box::new(IdedExpr {
            id: self.id(offset),
            expr,
        })
    }

    fn call(
        &mut self,
        offset: usize,
        name: &str,
        target: Option<Box<IdedExpr>>,
        args: Vec<Box<IdedExpr>>,
    ) -> Box<IdedExpr> {
        self.node(
            offset,
            Expr::Call(CallExpr {
                func_name: name.into(),
                target,
                args: args.into_iter().map(|node| *node).collect(),
            }),
        )
    }

    fn expression(&mut self) -> Result<Box<IdedExpr>, String> {
        let mut frames = vec![Frame::Expression];
        let mut values = Vec::new();
        while let Some(frame) = frames.pop() {
            match frame {
                Frame::Expression => {
                    self.depth += 1;
                    if self.depth > 250 {
                        return Err(self.error("expression recursion limit exceeded: 250"));
                    }
                    frames.extend([Frame::EndExpression, Frame::Conditional, Frame::Binary(1)]);
                }
                Frame::EndExpression => self.depth -= 1,
                Frame::Conditional => {
                    if self.current().kind == T::TokQuestion {
                        let question = self.advance();
                        frames.push(Frame::Truthy(values.pop().unwrap(), question.start));
                        frames.push(Frame::Binary(1));
                    }
                }
                Frame::Truthy(condition, offset) => {
                    self.expect(T::TokColon, "':'")?;
                    frames.push(Frame::Falsy(condition, values.pop().unwrap(), offset));
                    frames.push(Frame::Expression);
                }
                Frame::Falsy(condition, truthy, offset) => {
                    let falsy = values.pop().unwrap();
                    values.push(self.call(offset, "_?_:_", None, vec![condition, truthy, falsy]));
                }
                Frame::Binary(minimum) => {
                    frames.extend([Frame::BinaryTail(minimum), Frame::Unary]);
                }
                Frame::BinaryTail(minimum) => {
                    if let Some((precedence, name)) = self.binary_operator()
                        && precedence >= minimum
                    {
                        let operator = self.advance();
                        frames.push(Frame::BinaryRight(
                            minimum,
                            values.pop().unwrap(),
                            operator.start,
                            name,
                        ));
                        frames.push(Frame::Binary(precedence + 1));
                    }
                }
                Frame::BinaryRight(minimum, left, offset, name) => {
                    let right = values.pop().unwrap();
                    values.push(self.call(offset, name, None, vec![left, right]));
                    frames.push(Frame::BinaryTail(minimum));
                }
                Frame::Unary => {
                    let operator = self.current();
                    if matches!(operator.kind, T::TokMinus | T::TokExclamation) {
                        let mut count = 0;
                        while self.take(operator.kind) {
                            count += 1;
                        }
                        if operator.kind == T::TokMinus
                            && count % 2 == 1
                            && matches!(self.current().kind, T::TokInt | T::TokFloat)
                        {
                            let token = self.advance();
                            let literal = self.literal(token, true)?;
                            values.push(self.node(operator.start, Expr::Literal(literal)));
                            frames.push(Frame::Suffix);
                            continue;
                        }
                        if count % 2 == 1 {
                            frames.push(Frame::ApplyUnary(
                                operator.start,
                                if operator.kind == T::TokMinus {
                                    "-_"
                                } else {
                                    "!_"
                                },
                            ));
                        }
                    }
                    frames.extend([Frame::Suffix, Frame::Primary]);
                }
                Frame::ApplyUnary(offset, name) => {
                    let operand = values.pop().unwrap();
                    values.push(self.call(offset, name, None, vec![operand]));
                }
                Frame::Primary => {
                    let token = self.current();
                    match token.kind {
                        T::TokLeftParen => {
                            self.advance();
                            frames.extend([Frame::CloseParen, Frame::Expression]);
                        }
                        T::TokLeftBracket | T::TokLeftBrace => {
                            frames.push(self.collection_frame(None));
                        }
                        T::TokIdent | T::TokDot | T::TokReservedWord => {
                            let leading = self.take(T::TokDot);
                            if self.structure_ahead() {
                                let mut name = if leading {
                                    ".".to_owned()
                                } else {
                                    String::new()
                                };
                                name.push_str(&self.selector("type name")?);
                                while self.take(T::TokDot) {
                                    name.push('.');
                                    name.push_str(&self.selector("type name")?);
                                }
                                frames.push(self.collection_frame(Some(name)));
                                continue;
                            }
                            if self.current().kind == T::TokReservedWord && !leading {
                                return Err(self.error(&format!(
                                    "reserved identifier: {}",
                                    self.text(self.current())
                                )));
                            }
                            let ident = self.expect(T::TokIdent, "identifier")?;
                            if self.text(ident).starts_with('`') {
                                return Err(diagnostic(
                                    self.source,
                                    ident.start,
                                    "unexpected token 'EscapedIdent'",
                                ));
                            }
                            let name =
                                format!("{}{}", if leading { "." } else { "" }, self.text(ident));
                            if self.take(T::TokLeftParen) {
                                frames.push(Frame::Arguments(Call {
                                    offset: token.start,
                                    name,
                                    target: None,
                                    args: Vec::new(),
                                }));
                            } else {
                                values.push(self.node(token.start, Expr::Ident(name)));
                            }
                        }
                        T::TokInt
                        | T::TokUint
                        | T::TokFloat
                        | T::TokString
                        | T::TokBytes
                        | T::TokTrue
                        | T::TokFalse
                        | T::TokNull => {
                            self.advance();
                            let literal = self.literal(token, false)?;
                            values.push(self.node(token.start, Expr::Literal(literal)));
                        }
                        _ => {
                            return Err(
                                self.error(&format!("unexpected token {}", self.describe()))
                            );
                        }
                    }
                }
                Frame::CloseParen => {
                    self.expect(T::TokRightParen, "')'")?;
                }
                Frame::Suffix => {
                    if self.current().kind == T::TokDot {
                        let dot = self.advance();
                        if self.current().kind == T::TokQuestion {
                            return Err(self.error("unsupported syntax '.?'"));
                        }
                        let name = self.selector("field or method name")?;
                        let operand = values.pop().unwrap();
                        frames.push(Frame::Suffix);
                        if self.take(T::TokLeftParen) {
                            frames.push(Frame::Arguments(Call {
                                offset: dot.start,
                                name,
                                target: Some(operand),
                                args: Vec::new(),
                            }));
                        } else {
                            values.push(self.node(
                                dot.start,
                                Expr::Select(SelectExpr {
                                    operand,
                                    field: name,
                                    test: false,
                                }),
                            ));
                        }
                    } else if self.current().kind == T::TokLeftBracket {
                        let bracket = self.advance();
                        if self.current().kind == T::TokQuestion {
                            return Err(self.error("unsupported syntax '[?'"));
                        }
                        frames.push(Frame::Suffix);
                        frames.push(Frame::Index(values.pop().unwrap(), bracket.start));
                        frames.push(Frame::Expression);
                    }
                }
                Frame::Index(operand, offset) => {
                    self.expect(T::TokRightBracket, "']'")?;
                    let index = values.pop().unwrap();
                    values.push(self.call(offset, "_[_]", None, vec![operand, index]));
                }
                Frame::Arguments(call) => {
                    if self.take(T::TokRightParen) {
                        values.push(self.finish_call(call));
                    } else {
                        frames.extend([Frame::Argument(call), Frame::Expression]);
                    }
                }
                Frame::Argument(mut call) => {
                    call.args.push(*values.pop().unwrap());
                    if self.take(T::TokComma) {
                        frames.extend([Frame::Argument(call), Frame::Expression]);
                    } else {
                        self.expect(T::TokRightParen, "')'")?;
                        values.push(self.finish_call(call));
                    }
                }
                Frame::Collection(collection) => {
                    if self.current().kind == collection.close()
                        || (collection.elements.is_empty()
                            && collection.entries.is_empty()
                            && self.take(T::TokComma))
                    {
                        values.push(self.finish_collection(collection)?);
                        continue;
                    }
                    if self.current().kind == T::TokQuestion {
                        return Err(self.error("unsupported syntax '?'"));
                    }
                    let offset = self.current().start;
                    if collection.is_list() {
                        frames.push(Frame::Element(collection));
                    } else if collection.structure.is_some() {
                        let field = self.selector("field name")?;
                        self.expect(T::TokColon, "':'")?;
                        frames.push(Frame::StructValue(collection, offset, field));
                    } else {
                        frames.push(Frame::MapKey(collection, offset));
                    }
                    frames.push(Frame::Expression);
                }
                Frame::CollectionTail(collection) => {
                    if self.take(T::TokComma) {
                        frames.push(Frame::Collection(collection));
                    } else {
                        values.push(self.finish_collection(collection)?);
                    }
                }
                Frame::Element(mut collection) => {
                    collection.elements.push(*values.pop().unwrap());
                    frames.push(Frame::CollectionTail(collection));
                }
                Frame::MapKey(collection, offset) => {
                    self.expect(T::TokColon, "':'")?;
                    frames.push(Frame::MapValue(collection, offset, values.pop().unwrap()));
                    frames.push(Frame::Expression);
                }
                Frame::MapValue(mut collection, offset, key) => {
                    collection.entries.push(IdedEntryExpr {
                        id: self.id(offset),
                        expr: EntryExpr::MapEntry(MapEntryExpr {
                            key: *key,
                            value: *values.pop().unwrap(),
                            optional: false,
                        }),
                    });
                    frames.push(Frame::CollectionTail(collection));
                }
                Frame::StructValue(mut collection, offset, field) => {
                    collection.entries.push(IdedEntryExpr {
                        id: self.id(offset),
                        expr: EntryExpr::StructField(StructFieldExpr {
                            field,
                            value: *values.pop().unwrap(),
                            optional: false,
                        }),
                    });
                    frames.push(Frame::CollectionTail(collection));
                }
            }
        }
        assert_eq!(values.len(), 1);
        Ok(values.pop().unwrap())
    }

    fn binary_operator(&self) -> Option<(u8, &'static str)> {
        Some(match self.current().kind {
            T::TokLogicalOr => (1, "_||_"),
            T::TokLogicalAnd => (2, "_&&_"),
            T::TokEqualEqual => (3, "_==_"),
            T::TokExclamationEqual => (3, "_!=_"),
            T::TokLess => (3, "_<_"),
            T::TokLessEqual => (3, "_<=_"),
            T::TokGreater => (3, "_>_"),
            T::TokGreaterEqual => (3, "_>=_"),
            T::TokIn => (3, "@in"),
            T::TokPlus => (4, "_+_"),
            T::TokMinus => (4, "_-_"),
            T::TokAsterisk => (5, "_*_"),
            T::TokSlash => (5, "_/_"),
            T::TokPercent => (5, "_%_"),
            _ => return None,
        })
    }

    fn selector(&mut self, name: &str) -> Result<String, String> {
        if !matches!(self.current().kind, T::TokIdent | T::TokReservedWord) {
            return Err(self.error(&format!("expected {name}, found {}", self.describe())));
        }
        let token = self.advance();
        Ok(self.text(token).trim_matches('`').to_owned())
    }

    fn finish_call(&mut self, call: Call) -> Box<IdedExpr> {
        self.node(
            call.offset,
            Expr::Call(CallExpr {
                func_name: call.name,
                target: call.target,
                args: call.args,
            }),
        )
    }

    fn structure_ahead(&self) -> bool {
        let mut cursor = self.cursor;
        loop {
            if !matches!(self.tokens[cursor].kind, T::TokIdent | T::TokReservedWord)
                || self.text(self.tokens[cursor]).starts_with('`')
            {
                return false;
            }
            cursor += 1;
            if self.tokens[cursor].kind != T::TokDot {
                return self.tokens[cursor].kind == T::TokLeftBrace;
            }
            cursor += 1;
        }
    }

    fn literal(&self, token: Token, negative: bool) -> Result<LiteralValue, String> {
        let raw = self.text(token);
        match token.kind {
            T::TokInt | T::TokUint => {
                let raw = raw.trim_end_matches(['u', 'U']);
                let magnitude =
                    if let Some(hex) = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
                        u64::from_str_radix(hex, 16)
                    } else {
                        raw.parse::<u64>()
                    };
                let invalid = || {
                    diagnostic(
                        self.source,
                        token.start,
                        if token.kind == T::TokUint {
                            "invalid unsigned integer literal: value out of range"
                        } else {
                            "invalid int literal: value out of range"
                        },
                    )
                };
                let magnitude = magnitude.map_err(|_| invalid())?;
                if token.kind == T::TokUint {
                    return Ok(LiteralValue::UInt(magnitude.into()));
                }
                let integer = if negative && magnitude == 1_u64 << 63 {
                    i64::MIN
                } else {
                    let integer = i64::try_from(magnitude).map_err(|_| invalid())?;
                    if negative { -integer } else { integer }
                };
                Ok(LiteralValue::Int(integer.into()))
            }
            T::TokFloat => {
                let number = raw
                    .parse::<f64>()
                    .map_err(|error| diagnostic(self.source, token.start, &error.to_string()))?;
                Ok(LiteralValue::Double(
                    (if negative { -number } else { number }).into(),
                ))
            }
            T::TokTrue => Ok(LiteralValue::Boolean(true.into())),
            T::TokFalse => Ok(LiteralValue::Boolean(false.into())),
            T::TokNull => Ok(LiteralValue::Null),
            _ => self
                .literals
                .get(&token.start)
                .cloned()
                .ok_or_else(|| diagnostic(self.source, token.start, "invalid literal")),
        }
    }

    fn collection_frame(&mut self, structure: Option<String>) -> Frame {
        Frame::Collection(Collection {
            open: self.advance(),
            structure,
            elements: Vec::new(),
            entries: Vec::new(),
        })
    }

    fn finish_collection(&mut self, collection: Collection) -> Result<Box<IdedExpr>, String> {
        let list = collection.is_list();
        self.expect(collection.close(), if list { "']'" } else { "'}'" })?;
        Ok(self.node(
            collection.open.start,
            if let Some(type_name) = collection.structure {
                Expr::Struct(StructExpr {
                    type_name,
                    entries: collection.entries,
                })
            } else if list {
                Expr::List(ListExpr::new(collection.elements))
            } else {
                Expr::Map(MapExpr {
                    entries: collection.entries,
                })
            },
        ))
    }
}
