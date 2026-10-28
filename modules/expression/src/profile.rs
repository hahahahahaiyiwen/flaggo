use cel::ExecutionError;
use cel::common::ast::{Expr, IdedExpr, LiteralValue};
use flaggo_contract::ValueSchema;
use indexmap::IndexMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Kind {
    Dyn,
    Null,
    Bool,
    Int,
    Double,
    String,
    List,
    Map,
}

impl Kind {
    fn schema(schema: &str) -> Result<Self, String> {
        match schema {
            "null" => Ok(Self::Null),
            "boolean" => Ok(Self::Bool),
            "integer" => Ok(Self::Int),
            "number" => Ok(Self::Double),
            "string" => Ok(Self::String),
            "array" => Ok(Self::List),
            "object" => Ok(Self::Map),
            _ => Err(format!("unsupported schema type {schema}")),
        }
    }

    fn sized(self) -> bool {
        matches!(self, Self::String | Self::List | Self::Map)
    }
}

struct Checked {
    kind: Kind,
    cel_type: cel_core::CelType,
    cost: Option<usize>,
    minimum_cost: usize,
    size: Option<usize>,
}

struct Checker<'a> {
    schemas: &'a IndexMap<String, ValueSchema>,
    stored_types: Option<&'a std::collections::HashMap<i64, cel_core_proto::Type>>,
    parsed: Option<&'a crate::syntax::Parsed<'a>>,
}

impl Checker<'_> {
    fn stored_kind(&self, node: &IdedExpr) -> Result<Option<Kind>, String> {
        self.stored_types
            .map(|types| {
                let id = i64::try_from(node.id).map_err(|error| error.to_string())?;
                let kind = types
                    .get(&id)
                    .ok_or_else(|| "checked expression is missing type metadata".to_owned())?;
                super::checked::primitive(kind)
            })
            .transpose()
    }

    fn check(&self, node: &IdedExpr) -> Result<Checked, String> {
        let checked = match &node.expr {
            Expr::Literal(value) => {
                let (kind, size) = match value {
                    LiteralValue::Boolean(_) => (Kind::Bool, Some(1)),
                    LiteralValue::Double(_) => (Kind::Double, Some(1)),
                    LiteralValue::Int(_) => (Kind::Int, Some(1)),
                    LiteralValue::Null => (Kind::Null, Some(1)),
                    LiteralValue::String(value) => {
                        (Kind::String, Some(value.inner().encode_utf16().count()))
                    }
                    _ => return Err("unsupported literal".to_owned()),
                };
                Checked {
                    kind,
                    cel_type: match kind {
                        Kind::Null => cel_core::CelType::Null,
                        Kind::Bool => cel_core::CelType::Bool,
                        Kind::Int => cel_core::CelType::Int,
                        Kind::Double => cel_core::CelType::Double,
                        Kind::String => cel_core::CelType::String,
                        _ => unreachable!("literal containers are rejected by profile inspection"),
                    },
                    cost: Some(0),
                    minimum_cost: 0,
                    size,
                }
            }
            Expr::Select(select)
                if !select.test
                    && matches!(&select.operand.expr, Expr::Ident(name) if name == "attributes") =>
            {
                let kind = if select.field == "_random" {
                    Kind::Double
                } else {
                    let schema = self.schemas.get(&select.field).ok_or_else(|| {
                        format!(
                            "Expression references undeclared attribute '{}'.",
                            select.field,
                        )
                    })?;
                    self.stored_kind(node)?
                        .map_or_else(|| Kind::schema(&schema.kind), Ok)?
                };
                Checked {
                    kind,
                    cel_type: if select.field == "_random" {
                        cel_core::CelType::Double
                    } else {
                        super::checked::schema_type(&self.schemas[&select.field])
                    },
                    cost: Some(2),
                    minimum_cost: 2,
                    size: if kind.sized() { None } else { Some(1) },
                }
            }
            Expr::Call(call) if call.target.is_none() => {
                let mut arguments = Vec::new();
                let mut errors = Vec::new();
                for argument in &call.args {
                    match self.check(argument) {
                        Ok(checked) => arguments.push(checked),
                        Err(error) => errors.push(error),
                    }
                }
                if !errors.is_empty() {
                    return Err(errors.join("; "));
                }
                self.check_call(node, &call.func_name, &arguments, self.stored_kind(node)?)?
            }
            Expr::Ident(name) => {
                return Err(format!(
                    "Identifier '{name}' is not available; use attributes.<name>.",
                ));
            }
            Expr::Select(_) => {
                return Err("Only direct attributes.<name> selection is supported.".to_owned());
            }
            Expr::List(_) => {
                return Err("list literals are not supported by flaggo.cel/v1.".to_owned());
            }
            Expr::Map(_) => {
                return Err("map literals are not supported by flaggo.cel/v1.".to_owned());
            }
            Expr::Struct(_) => {
                return Err("message literals are not supported by flaggo.cel/v1.".to_owned());
            }
            _ => return Err("expression is outside flaggo.cel/v1".to_owned()),
        };
        Ok(checked)
    }

    fn check_call(
        &self,
        node: &IdedExpr,
        name: &str,
        args: &[Checked],
        stored_kind: Option<Kind>,
    ) -> Result<Checked, String> {
        let arity = arity(name).ok_or_else(|| unsupported_function(name))?;
        if args.len() != arity {
            return Err(self.overload_error(node, name, args));
        }
        let kinds = args
            .iter()
            .map(|argument| argument.kind)
            .collect::<Vec<_>>();
        let numeric = |kind| matches!(kind, Kind::Int | Kind::Double);
        if stored_kind.is_none()
            && name == "_?_:_"
            && !unifiable_types(&args[1].cel_type, &args[2].cel_type)
        {
            return Err(self.overload_error(node, name, args));
        }
        let inferred = if stored_kind.is_none()
            && (name == "_?_:_"
                || kinds.contains(&Kind::Dyn)
                || (kinds == [Kind::Null, Kind::Null]
                    && matches!(name, "_+_" | "_-_" | "_<_" | "_<=_" | "_>_" | "_>=_")))
        {
            Some(
                super::checked::nullable_call_type(
                    node,
                    &args
                        .iter()
                        .map(|arg| arg.cel_type.clone())
                        .collect::<Vec<_>>(),
                )
                .map_err(|_| self.overload_error(node, name, args))?,
            )
        } else {
            None
        };
        let kind = if let Some(kind) = stored_kind {
            kind
        } else if let Some(kind) = &inferred {
            match kind {
                cel_core::CelType::Dyn => Kind::Dyn,
                cel_core::CelType::Null => Kind::Null,
                cel_core::CelType::Bool => Kind::Bool,
                cel_core::CelType::Int => Kind::Int,
                cel_core::CelType::Double => Kind::Double,
                cel_core::CelType::String => Kind::String,
                cel_core::CelType::List(_) => Kind::List,
                cel_core::CelType::Map(_, _) => Kind::Map,
                _ => return Err(self.overload_error(node, name, args)),
            }
        } else {
            match (name, kinds.as_slice()) {
                ("!_", [Kind::Bool]) => Kind::Bool,
                ("-_", [kind]) if numeric(*kind) => *kind,
                ("_&&_" | "_||_", [Kind::Bool, Kind::Bool]) => Kind::Bool,
                ("_==_" | "_!=_", [a, b]) if a == b => Kind::Bool,
                ("_<_" | "_<=_" | "_>_" | "_>=_", [a, b])
                    if a == b && (numeric(*a) || *a == Kind::String) =>
                {
                    Kind::Bool
                }
                ("_+_" | "_-_" | "_*_" | "_/_", [a, b]) if a == b && numeric(*a) => *a,
                ("_+_", [Kind::String, Kind::String]) => Kind::String,
                ("_+_", [Kind::List, Kind::List]) => Kind::List,
                ("_%_", [Kind::Int, Kind::Int]) => Kind::Int,
                ("bool", [Kind::Bool | Kind::String]) => Kind::Bool,
                ("int", [Kind::Int | Kind::Double | Kind::String | Kind::Null]) => Kind::Int,
                ("double", [Kind::Int | Kind::Double | Kind::String]) => Kind::Double,
                ("string", [Kind::Int | Kind::Double | Kind::String | Kind::Bool | Kind::Null]) => {
                    Kind::String
                }
                ("size", [Kind::String | Kind::List | Kind::Map]) => Kind::Int,
                _ => return Err(self.overload_error(node, name, args)),
            }
        };
        let invocation_cost = if name == "_+_" && kind.sized() {
            args[0].size.zip(args[1].size).map(|(a, b)| a + b)
        } else if matches!(name, "_==_" | "_!=_") && kinds[0].sized() {
            args[0].size.zip(args[1].size).map(|(a, b)| a.max(b))
        } else {
            Some(1)
        }
        .map(|cost| cost.max(1));
        let size = if name == "_+_" && kind.sized() {
            args[0].size.zip(args[1].size).map(|(a, b)| a + b)
        } else if kind.sized() {
            None
        } else {
            Some(1)
        };
        Ok(Checked {
            kind,
            cel_type: inferred.unwrap_or_else(|| match kind {
                Kind::List | Kind::Map => {
                    let operands = if name == "_?_:_" { &args[1..] } else { args };
                    operands
                        .iter()
                        .skip(1)
                        .fold(operands[0].cel_type.clone(), |left, right| {
                            join_types(&left, &right.cel_type)
                        })
                }
                Kind::Dyn => cel_core::CelType::Dyn,
                Kind::Null => cel_core::CelType::Null,
                Kind::Bool => cel_core::CelType::Bool,
                Kind::Int => cel_core::CelType::Int,
                Kind::Double => cel_core::CelType::Double,
                Kind::String => cel_core::CelType::String,
            }),
            cost: args
                .iter()
                .map(|argument| argument.cost)
                .chain([invocation_cost])
                .try_fold(0_usize, |total, cost| Some(total.saturating_add(cost?))),
            minimum_cost: args
                .iter()
                .map(|argument| argument.minimum_cost)
                .sum::<usize>()
                + 1,
            size,
        })
    }

    fn overload_error(&self, node: &IdedExpr, name: &str, args: &[Checked]) -> String {
        let signature = args
            .iter()
            .map(|argument| type_name(&argument.cel_type))
            .collect::<Vec<_>>()
            .join(", ");
        let message = format!("found no matching overload for '{name}' applied to '({signature})'");
        self.parsed.map_or_else(
            || message.clone(),
            |parsed| parsed.diagnostic(node, &message),
        )
    }
}

fn unifiable_types(left: &cel_core::CelType, right: &cel_core::CelType) -> bool {
    use cel_core::CelType;
    match (left, right) {
        (CelType::Dyn, _) | (_, CelType::Dyn) => true,
        (CelType::List(left), CelType::List(right)) => unifiable_types(left, right),
        (CelType::Map(left_key, left_value), CelType::Map(right_key, right_value)) => {
            unifiable_types(left_key, right_key) && unifiable_types(left_value, right_value)
        }
        _ => left == right,
    }
}

fn join_types(left: &cel_core::CelType, right: &cel_core::CelType) -> cel_core::CelType {
    use cel_core::CelType;
    if left == right {
        return left.clone();
    }
    match (left, right) {
        (CelType::List(left), CelType::List(right)) => CelType::list(join_types(left, right)),
        (CelType::Map(left_key, left_value), CelType::Map(right_key, right_value)) => CelType::map(
            join_types(left_key, right_key),
            join_types(left_value, right_value),
        ),
        _ => CelType::Dyn,
    }
}

fn type_name(kind: &cel_core::CelType) -> String {
    use cel_core::CelType;
    match kind {
        CelType::Null => "null_type".to_owned(),
        CelType::List(item) => format!("list({})", type_name(item)),
        CelType::Map(key, value) => format!("map({}, {})", type_name(key), type_name(value)),
        _ => kind.to_string(),
    }
}

fn arity(name: &str) -> Option<usize> {
    match name {
        "_?_:_" => Some(3),
        "!_" | "-_" | "bool" | "int" | "double" | "string" | "size" => Some(1),
        name if binary(name).is_some() => Some(2),
        _ => None,
    }
}

fn unsupported_function(name: &str) -> String {
    format!("Function or operator '{name}' is not supported by flaggo.cel/v1.")
}

fn inspect(
    expression: &IdedExpr,
    schemas: &IndexMap<String, ValueSchema>,
    stored: bool,
) -> Result<(), String> {
    let mut pending = vec![(expression, 1)];
    let mut references = indexmap::IndexSet::new();
    let mut nodes = 0;
    let mut maximum = 0;
    while let Some((node, depth)) = pending.pop() {
        nodes += 1;
        maximum = maximum.max(depth);
        if depth > crate::MAXIMUM_AST_DEPTH + 1 {
            continue;
        }
        match &node.expr {
            Expr::Literal(LiteralValue::Bytes(_)) => {
                return Err("bytes literals are not supported by flaggo.cel/v1.".into());
            }
            Expr::Literal(LiteralValue::UInt(_)) => {
                return Err("uint literals are not supported by flaggo.cel/v1.".into());
            }
            Expr::Literal(_) => {}
            Expr::Select(select)
                if !select.test
                    && matches!(&select.operand.expr, Expr::Ident(name) if name == "attributes") =>
            {
                nodes += 1;
                maximum = maximum.max(depth + 1);
                references.insert(select.field.as_str());
            }
            Expr::Call(call) => {
                if call.target.is_some() || arity(&call.func_name).is_none() {
                    return Err(unsupported_function(&call.func_name));
                }
                pending.extend(call.args.iter().rev().map(|argument| (argument, depth + 1)));
            }
            Expr::Ident(name) => {
                return Err(format!(
                    "Identifier '{name}' is not available; use attributes.<name>."
                ));
            }
            Expr::Select(_) => {
                return Err("Only direct attributes.<name> selection is supported.".into());
            }
            Expr::List(_) => {
                return Err("list literals are not supported by flaggo.cel/v1.".into());
            }
            Expr::Map(_) => {
                return Err("map literals are not supported by flaggo.cel/v1.".into());
            }
            Expr::Struct(_) => {
                return Err("message literals are not supported by flaggo.cel/v1.".into());
            }
            _ => {
                return Err("comprehensions and macros are not supported by flaggo.cel/v1.".into());
            }
        }
    }
    if stored && (maximum > crate::MAXIMUM_AST_DEPTH || nodes > crate::MAXIMUM_AST_NODES) {
        return Err("Checked expression exceeds the flaggo.cel/v1 AST limits.".into());
    }
    if maximum > crate::MAXIMUM_AST_DEPTH {
        return Err(format!(
            "Expression exceeds the maximum AST depth of {}.",
            crate::MAXIMUM_AST_DEPTH
        ));
    }
    if nodes > crate::MAXIMUM_AST_NODES {
        return Err(format!(
            "Expression exceeds the maximum AST node count of {}.",
            crate::MAXIMUM_AST_NODES
        ));
    }
    for field in references {
        if field != "_random" && schemas.get(field).is_none() {
            return Err(format!(
                "{} references undeclared attribute '{field}'.",
                if stored {
                    "Checked expression"
                } else {
                    "Expression"
                },
            ));
        }
    }
    Ok(())
}

fn float_string(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value == f64::INFINITY {
        return "+Inf".to_owned();
    }
    if value == f64::NEG_INFINITY {
        return "-Inf".to_owned();
    }
    let scientific = format!("{value:e}");
    let (digits, exponent) = scientific
        .split_once('e')
        .expect("finite scientific formatting contains an exponent");
    let exponent: i32 = exponent.parse().expect("scientific exponent is numeric");
    if !(-4..6).contains(&exponent) {
        let sign = if exponent < 0 { '-' } else { '+' };
        format!("{digits}e{sign}{:02}", exponent.unsigned_abs())
    } else {
        value.to_string()
    }
}

pub(crate) fn string_value(value: cel::Value) -> Result<cel::Value, ExecutionError> {
    let text = match value {
        cel::Value::Float(value) => float_string(value),
        cel::Value::Int(value) => value.to_string(),
        cel::Value::Bool(value) => value.to_string(),
        cel::Value::String(value) => value.as_str().to_owned(),
        _ => {
            return Err(ExecutionError::function_error(
                "string",
                "unsupported value type",
            ));
        }
    };
    Ok(cel::Value::String(text.into()))
}

pub(crate) fn runtime_expression(node: &IdedExpr) -> IdedExpr {
    let mut result = node.clone();
    if let Expr::Call(call) = &mut result.expr {
        call.args = call.args.iter().map(runtime_expression).collect();
        if call.func_name == "string" {
            call.func_name = "__flaggo_string".to_owned();
        }
    }
    result
}

fn binary(name: &str) -> Option<(&'static str, u8)> {
    Some(match name {
        "_||_" => ("||", 2),
        "_&&_" => ("&&", 3),
        "_==_" => ("==", 4),
        "_!=_" => ("!=", 4),
        "_<_" => ("<", 4),
        "_<=_" => ("<=", 4),
        "_>_" => (">", 4),
        "_>=_" => (">=", 4),
        "_+_" => ("+", 5),
        "_-_" => ("-", 5),
        "_*_" => ("*", 6),
        "_/_" => ("/", 6),
        "_%_" => ("%", 6),
        _ => return None,
    })
}

fn quote(value: &str) -> String {
    let mut result = String::from("\"");
    for character in value.chars() {
        match character {
            '\u{7}' => result.push_str("\\a"),
            '\u{8}' => result.push_str("\\b"),
            '\u{c}' => result.push_str("\\f"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            '\u{b}' => result.push_str("\\v"),
            '\\' => result.push_str("\\\\"),
            '"' => result.push_str("\\\""),
            value if value.is_control() => {
                use std::fmt::Write;
                write!(&mut result, "\\u{:04x}", u32::from(value))
                    .expect("writing to a String cannot fail");
            }
            value => result.push(value),
        }
    }
    result.push('"');
    result
}

pub(crate) fn canonical(node: &IdedExpr, parent: u8, right: bool) -> Result<String, String> {
    use cel::common::ast::EntryExpr;
    enum Piece<'a> {
        Node(&'a IdedExpr, u8, bool),
        Text(String),
    }
    use Piece::{Node, Text};
    fn field(name: &str) -> String {
        if !name.is_empty()
            && name.bytes().enumerate().all(|(index, byte)| {
                byte.is_ascii_alphabetic() || byte == b'_' || index > 0 && byte.is_ascii_digit()
            })
        {
            name.to_owned()
        } else {
            format!("`{name}`")
        }
    }
    let mut result = String::new();
    let mut pending = vec![Node(node, parent, right)];
    while let Some(piece) = pending.pop() {
        let Node(node, parent, right) = piece else {
            if let Text(text) = piece {
                result.push_str(&text);
            }
            continue;
        };
        let mut pieces = Vec::new();
        match &node.expr {
            Expr::Literal(value) => {
                let text = match value {
                    LiteralValue::Boolean(value) => value.inner().to_string(),
                    LiteralValue::Int(value) => value.inner().to_string(),
                    LiteralValue::UInt(value) => format!("{}u", value.inner()),
                    LiteralValue::Double(value) => {
                        let mut text = float_string(*value.inner());
                        if value.inner().is_finite() && !text.contains('.') && !text.contains('e') {
                            text.push_str(".0");
                        }
                        text
                    }
                    LiteralValue::String(value) => quote(value.inner()),
                    LiteralValue::Bytes(value) => {
                        let mut text = String::from("b\"");
                        use std::fmt::Write;
                        for byte in value.inner().iter() {
                            match byte {
                                0x20..=0x7e if *byte != b'\\' && *byte != b'"' => {
                                    text.push(char::from(*byte));
                                }
                                _ => write!(&mut text, "\\x{byte:02x}")
                                    .expect("writing to a String cannot fail"),
                            }
                        }
                        text.push('"');
                        text
                    }
                    LiteralValue::Null => "null".to_owned(),
                };
                pieces.push(Text(text));
            }
            Expr::Ident(name) => pieces.push(Text(name.clone())),
            Expr::Select(select) => {
                if select.test {
                    pieces.push(Text("has(".to_owned()));
                }
                pieces.push(Node(&select.operand, 8, false));
                pieces.push(Text(format!(".{}", field(&select.field))));
                if select.test {
                    pieces.push(Text(")".to_owned()));
                }
            }
            Expr::Call(call) => {
                if let Some((symbol, precedence)) = binary(&call.func_name).or_else(|| {
                    (call.func_name == "@in" || call.func_name == "_in_").then_some(("in", 4))
                }) {
                    if call.args.len() != 2 {
                        return Err("invalid binary expression".to_owned());
                    }
                    let parentheses = parent > precedence || parent == precedence && right;
                    if parentheses {
                        pieces.push(Text("(".to_owned()));
                    }
                    pieces.push(Node(&call.args[0], precedence, false));
                    pieces.push(Text(format!(" {symbol} ")));
                    pieces.push(Node(&call.args[1], precedence, true));
                    if parentheses {
                        pieces.push(Text(")".to_owned()));
                    }
                } else if call.func_name == "_?_:_" && call.args.len() == 3 {
                    let parentheses = parent > 1 || parent == 1 && !right;
                    if parentheses {
                        pieces.push(Text("(".to_owned()));
                    }
                    pieces.push(Node(&call.args[0], 2, false));
                    pieces.push(Text(" ? ".to_owned()));
                    pieces.push(Node(&call.args[1], 2, false));
                    pieces.push(Text(" : ".to_owned()));
                    pieces.push(Node(&call.args[2], 1, true));
                    if parentheses {
                        pieces.push(Text(")".to_owned()));
                    }
                } else if matches!(call.func_name.as_str(), "!_" | "-_") && call.args.len() == 1 {
                    pieces.push(Text(
                        if call.func_name == "!_" { "!" } else { "-" }.to_owned(),
                    ));
                    pieces.push(Node(&call.args[0], 7, false));
                } else if call.func_name == "_[_]" && call.args.len() == 2 {
                    pieces.push(Node(&call.args[0], 8, false));
                    pieces.push(Text("[".to_owned()));
                    pieces.push(Node(&call.args[1], 0, false));
                    pieces.push(Text("]".to_owned()));
                } else {
                    if let Some(target) = &call.target {
                        pieces.push(Node(target, 8, false));
                        pieces.push(Text(".".to_owned()));
                    }
                    pieces.push(Text(format!("{}(", call.func_name)));
                    for (index, argument) in call.args.iter().enumerate() {
                        if index > 0 {
                            pieces.push(Text(", ".to_owned()));
                        }
                        pieces.push(Node(argument, 0, false));
                    }
                    pieces.push(Text(")".to_owned()));
                }
            }
            Expr::List(list) => {
                pieces.push(Text("[".to_owned()));
                for (index, element) in list.elements.iter().enumerate() {
                    if index > 0 {
                        pieces.push(Text(", ".to_owned()));
                    }
                    pieces.push(Node(element, 0, false));
                }
                pieces.push(Text("]".to_owned()));
            }
            Expr::Map(map) => {
                pieces.push(Text("{".to_owned()));
                for (index, entry) in map.entries.iter().enumerate() {
                    let EntryExpr::MapEntry(entry) = &entry.expr else {
                        return Err("invalid map entry".to_owned());
                    };
                    if index > 0 {
                        pieces.push(Text(", ".to_owned()));
                    }
                    pieces.push(Node(&entry.key, 0, false));
                    pieces.push(Text(": ".to_owned()));
                    pieces.push(Node(&entry.value, 0, false));
                }
                pieces.push(Text("}".to_owned()));
            }
            Expr::Struct(value) => {
                pieces.push(Text(format!("{}{{", value.type_name)));
                for (index, entry) in value.entries.iter().enumerate() {
                    let EntryExpr::StructField(entry) = &entry.expr else {
                        return Err("invalid message entry".to_owned());
                    };
                    if index > 0 {
                        pieces.push(Text(", ".to_owned()));
                    }
                    pieces.push(Text(format!("{}: ", field(&entry.field))));
                    pieces.push(Node(&entry.value, 0, false));
                }
                pieces.push(Text("}".to_owned()));
            }
            Expr::Comprehension(_) => {
                return Err(
                    "syntax-only macro canonicalization has not reached reference parity"
                        .to_owned(),
                );
            }
            Expr::Unspecified => return Err("missing expression".to_owned()),
        }
        pending.extend(pieces.into_iter().rev());
    }
    Ok(result)
}

pub(crate) fn check(
    expression: &IdedExpr,
    schemas: &IndexMap<String, ValueSchema>,
    result_schema: &flaggo_contract::ValueSchema,
    purpose: flaggo_contract::validation::ExpressionPurpose,
    stored: Option<&super::CheckedExpression>,
    stored_types: Option<&std::collections::HashMap<i64, cel_core_proto::Type>>,
    parsed: Option<&crate::syntax::Parsed<'_>>,
) -> Result<(), String> {
    inspect(expression, schemas, stored.is_some())?;
    let checker = Checker {
        schemas,
        stored_types,
        parsed,
    };
    let actual = checker.check(expression)?;
    let predicate = purpose == flaggo_contract::validation::ExpressionPurpose::Predicate;
    let expected = Kind::schema(if predicate {
        "boolean"
    } else {
        &result_schema.kind
    })?;
    if actual.kind != expected && !(actual.kind == Kind::Int && expected == Kind::Double) {
        return Err(format!(
            "Expression has type '{}', expected '{}'.",
            type_name(&actual.cel_type),
            if predicate {
                "bool".to_owned()
            } else {
                type_name(&super::checked::schema_type(result_schema))
            },
        ));
    }
    if let Some(stored) = stored {
        let kind = checker
            .stored_kind(expression)?
            .ok_or_else(|| "checked expression is missing its result type".to_owned())?;
        if kind != expected && !(kind == Kind::Int && expected == Kind::Double) {
            return Err("checked result type does not match".to_owned());
        }
        if canonical(expression, 0, false)? != stored.canonical_expression {
            return Err("checked canonical expression does not match".to_owned());
        }
    }
    if actual
        .cost
        .is_none_or(|cost| cost > crate::MAXIMUM_STATIC_COST)
    {
        return Err(format!(
            "{} static cost [{}, {}] exceeds {}.",
            if stored.is_some() {
                "Checked expression"
            } else {
                "Expression"
            },
            actual.minimum_cost,
            actual
                .cost
                .map_or_else(|| "unbounded".to_owned(), |cost| cost.to_string()),
            crate::MAXIMUM_STATIC_COST,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::float_string;
    use crate::syntax::parse;

    #[test]
    fn parser_depth_matches_the_reference_without_counting_quoted_or_commented_tokens() {
        assert!(parse(&format!("{}1{}", "(".repeat(249), ")".repeat(249))).is_ok());
        assert!(parse(&format!("{}1{}", "(".repeat(250), ")".repeat(250))).is_err());
        for source in [
            format!("'{}'", "(".repeat(300)),
            format!("r'{}'", "(".repeat(300)),
            format!("'''{}'''", "(".repeat(300)),
            format!("1 // {}", "(".repeat(300)),
        ] {
            assert!(parse(&source).is_ok(), "{source}");
        }
    }

    #[test]
    fn string_conversion_preserves_go_general_format_boundaries() {
        for (number, expected) in [
            (0.0, "0"),
            (-0.0, "-0"),
            (1e-5, "1e-05"),
            (1e-4, "0.0001"),
            (999999.0, "999999"),
            (1e6, "1e+06"),
            (1e20, "1e+20"),
        ] {
            assert_eq!(float_string(number), expected);
        }
    }
}
