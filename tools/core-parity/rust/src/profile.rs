use super::{Request, cel_input};
use cel::common::ast::{Expr, IdedExpr, LiteralValue};
use cel::{Context, Env, ExecutionError};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Kind {
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
    cost: usize,
    size: Option<usize>,
}

struct Checker<'a> {
    schemas: &'a Value,
    nodes: usize,
    stored_types: Option<&'a std::collections::HashMap<i64, cel_core_proto::Type>>,
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

    fn check(&mut self, node: &IdedExpr, depth: usize) -> Result<Checked, String> {
        self.nodes += 1;
        if depth > 32 || self.nodes > 256 {
            return Err("expression exceeds AST depth/node limits".to_owned());
        }
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
                    cost: 0,
                    size,
                }
            }
            Expr::Select(select)
                if !select.test
                    && matches!(&select.operand.expr, Expr::Ident(name) if name == "attributes") =>
            {
                self.nodes += 1;
                if depth + 1 > 32 || self.nodes > 256 {
                    return Err("expression exceeds AST depth/node limits".to_owned());
                }
                let kind = if select.field == "_random" {
                    Kind::Double
                } else {
                    let schema = self.schemas[&select.field]["type"]
                        .as_str()
                        .ok_or_else(|| format!("undeclared attribute {}", select.field))?;
                    self.stored_kind(node)?
                        .map_or_else(|| Kind::schema(schema), Ok)?
                };
                Checked {
                    kind,
                    cost: 2,
                    size: if kind.sized() { None } else { Some(1) },
                }
            }
            Expr::Call(call) if call.target.is_none() => {
                let arguments = call
                    .args
                    .iter()
                    .map(|argument| self.check(argument, depth + 1))
                    .collect::<Result<Vec<_>, _>>()?;
                self.check_call(&call.func_name, &arguments, self.stored_kind(node)?)?
            }
            _ => return Err("expression is outside flaggo.cel/v1".to_owned()),
        };
        if checked.cost > 1000 {
            return Err("expression exceeds static cost limit".to_owned());
        }
        Ok(checked)
    }

    fn check_call(
        &self,
        name: &str,
        args: &[Checked],
        stored_kind: Option<Kind>,
    ) -> Result<Checked, String> {
        let arity = match name {
            "_?_:_" => 3,
            "!_" | "-_" | "bool" | "int" | "double" | "string" | "size" => 1,
            name if binary(name).is_some() => 2,
            _ => return Err(format!("unsupported function {name}")),
        };
        if args.len() != arity {
            return Err(format!("invalid arity for {name}"));
        }
        let kinds = args
            .iter()
            .map(|argument| argument.kind)
            .collect::<Vec<_>>();
        let numeric = |kind| matches!(kind, Kind::Int | Kind::Double);
        let kind = if let Some(kind) = stored_kind {
            kind
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
                ("_?_:_", [Kind::Bool, a, b]) if a == b => *a,
                ("bool", [Kind::Bool | Kind::String]) => Kind::Bool,
                ("int", [Kind::Int | Kind::Double | Kind::String | Kind::Null]) => Kind::Int,
                ("double", [Kind::Int | Kind::Double | Kind::String]) => Kind::Double,
                ("string", [Kind::Int | Kind::Double | Kind::String | Kind::Bool | Kind::Null]) => {
                    Kind::String
                }
                ("size", [Kind::String | Kind::List | Kind::Map]) => Kind::Int,
                _ => return Err(format!("unsupported overload {name} for {kinds:?}")),
            }
        };
        let invocation_cost = if name == "_+_" && kind.sized() {
            args[0].size.zip(args[1].size).map(|(a, b)| a + b)
        } else if matches!(name, "_==_" | "_!=_") && kinds[0].sized() {
            args[0].size.zip(args[1].size).map(|(a, b)| a.max(b))
        } else {
            Some(1)
        }
        .ok_or_else(|| "expression has unbounded static cost".to_owned())?
        .max(1);
        let size = if name == "_+_" && kind.sized() {
            args[0].size.zip(args[1].size).map(|(a, b)| a + b)
        } else if kind.sized() {
            None
        } else {
            Some(1)
        };
        Ok(Checked {
            kind,
            cost: args.iter().map(|argument| argument.cost).sum::<usize>() + invocation_cost,
            size,
        })
    }
}

fn check_parser_depth(source: &str) -> Result<(), String> {
    use cel::parser::pratt_parser::{Lexer, TokenKind};
    let mut lexer = Lexer::new(source);
    let mut parentheses = Vec::new();
    let mut depth = 1;
    loop {
        match lexer.lex().kind {
            TokenKind::TokEnd => return Ok(()),
            TokenKind::TokError => return Err("invalid expression token".to_owned()),
            TokenKind::TokLeftParen => {
                parentheses.push(depth);
                depth += 1;
            }
            TokenKind::TokRightParen => {
                depth = parentheses.pop().ok_or("unmatched closing parenthesis")?;
            }
            TokenKind::TokColon => depth += 1,
            TokenKind::TokComma => depth = parentheses.last().copied().unwrap_or(0) + 1,
            _ => {}
        }
        // Celly counts ParseExpr nesting, including grouping eliminated by PrattParser.
        if depth > 250 {
            return Err("expression exceeds the parser recursion limit of 250".to_owned());
        }
    }
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

fn string_value(value: cel::Value) -> Result<cel::Value, ExecutionError> {
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

fn runtime_expression(node: &IdedExpr) -> IdedExpr {
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

fn canonical(node: &IdedExpr, parent: u8, right: bool) -> String {
    match &node.expr {
        Expr::Literal(value) => match value {
            LiteralValue::Boolean(value) => value.inner().to_string(),
            LiteralValue::Int(value) => value.inner().to_string(),
            LiteralValue::Double(value) => {
                let mut text = float_string(*value.inner());
                if !text.contains('.') && !text.contains('e') {
                    text.push_str(".0");
                }
                text
            }
            LiteralValue::String(value) => quote(value.inner()),
            LiteralValue::Null => "null".to_owned(),
            _ => unreachable!("profile validation rejects this literal"),
        },
        Expr::Select(select) => {
            let field = &select.field;
            if !field.is_empty()
                && field.bytes().enumerate().all(|(index, byte)| {
                    byte.is_ascii_alphabetic() || byte == b'_' || index > 0 && byte.is_ascii_digit()
                })
            {
                format!("attributes.{field}")
            } else {
                format!("attributes.`{field}`")
            }
        }
        Expr::Call(call) => {
            if let Some((symbol, precedence)) = binary(&call.func_name) {
                let body = format!(
                    "{} {symbol} {}",
                    canonical(&call.args[0], precedence, false),
                    canonical(&call.args[1], precedence, true)
                );
                return if parent > precedence || parent == precedence && right {
                    format!("({body})")
                } else {
                    body
                };
            }
            if call.func_name == "_?_:_" {
                let body = format!(
                    "{} ? {} : {}",
                    canonical(&call.args[0], 2, false),
                    canonical(&call.args[1], 2, false),
                    canonical(&call.args[2], 1, true)
                );
                return if parent > 1 || parent == 1 && !right {
                    format!("({body})")
                } else {
                    body
                };
            }
            if matches!(call.func_name.as_str(), "!_" | "-_") {
                let symbol = if call.func_name == "!_" { "!" } else { "-" };
                return format!("{symbol}{}", canonical(&call.args[0], 7, false));
            }
            format!(
                "{}({})",
                call.func_name,
                call.args
                    .iter()
                    .map(|arg| canonical(arg, 0, false))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        _ => unreachable!("profile validation rejects this expression"),
    }
}

pub fn evaluate(
    request: Request,
    environment: &Arc<Env>,
) -> Result<Value, Box<dyn std::error::Error>> {
    let compiled = (|| {
        if request.checked.is_none()
            && (request.expression.trim().is_empty() || request.expression.len() > 4096)
        {
            return Err("expression is empty or exceeds 4096 bytes".to_owned());
        }
        let (expression, stored_types) = if let Some(stored) = &request.checked {
            let (expression, types) = super::checked::decode(stored)?;
            (expression, Some(types))
        } else {
            check_parser_depth(&request.expression)?;
            (
                cel::parser::PrattParser::default()
                    .enable_optional_syntax(false)
                    .enable_ident_escape_syntax(true)
                    .parse(&request.expression)
                    .map_err(|error| error.to_string())?,
                None,
            )
        };
        let mut checker = Checker {
            schemas: &request.schemas,
            nodes: 0,
            stored_types: stored_types.as_ref(),
        };
        let actual = checker.check(&expression, 1)?;
        let expected = Kind::schema(&request.result_type)?;
        if actual.kind != expected && !(actual.kind == Kind::Int && expected == Kind::Double) {
            return Err("expression result type does not match the schema".to_owned());
        }
        if let Some(stored) = &request.checked {
            let kind = checker
                .stored_kind(&expression)?
                .ok_or_else(|| "checked expression is missing its result type".to_owned())?;
            if kind != expected && !(kind == Kind::Int && expected == Kind::Double) {
                return Err("checked result type does not match".to_owned());
            }
            if canonical(&expression, 0, false) != stored.canonical_expression {
                return Err("checked canonical expression does not match".to_owned());
            }
        }
        Ok(expression)
    })();
    let expression = match compiled {
        Ok(expression) => expression,
        Err(error) => {
            return Ok(json!({"id": request.id, "status": "compile-error", "message": error}));
        }
    };
    let canonical = canonical(&expression, 0, false);
    let checked = if request.emit_checked && request.checked.is_none() {
        match super::checked::compile(
            &expression,
            &request.expression,
            &request.schemas,
            canonical.clone(),
        ) {
            Ok(checked) => Some(checked),
            Err(error) => {
                return Ok(json!({"id": request.id, "status": "compile-error", "message": error}));
            }
        }
    } else {
        request.checked
    };
    if request.compile_only {
        return Ok(
            json!({"id": request.id, "status": "compiled", "canonical": canonical, "checked": checked}),
        );
    }
    let mut context = Context::with_env(Arc::clone(environment));
    context.add_variable_from_value("attributes", cel_input(&request.attributes)?);
    context.add_function("__flaggo_string", string_value)?;
    let runtime = runtime_expression(&expression);
    let value = match cel::Value::resolve(&runtime, &context) {
        Ok(value) => value,
        Err(error) => {
            return Ok(json!({
                "id": request.id, "status": "evaluation-error", "message": error.to_string(),
                "checked": checked
            }));
        }
    };
    if matches!(&value, cel::Value::Float(number) if !number.is_finite()) {
        return Ok(json!({
            "id": request.id, "status": "runtime-fault", "message": "non-finite JSON result", "checked": checked
        }));
    }
    let result = value
        .json()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    Ok(
        json!({"id": request.id, "status": "value", "value": result, "canonical": canonical, "checked": checked}),
    )
}

#[cfg(test)]
mod tests {
    use super::{check_parser_depth, float_string};

    #[test]
    fn parser_depth_matches_the_reference_without_counting_quoted_or_commented_tokens() {
        assert!(check_parser_depth(&format!("{}1{}", "(".repeat(249), ")".repeat(249))).is_ok());
        assert!(check_parser_depth(&format!("{}1{}", "(".repeat(250), ")".repeat(250))).is_err());
        for source in [
            format!("'{}'", "(".repeat(300)),
            format!("r'{}'", "(".repeat(300)),
            format!("'''{}'''", "(".repeat(300)),
            format!("1 // {}", "(".repeat(300)),
        ] {
            assert!(check_parser_depth(&source).is_ok(), "{source}");
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
