use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use cel::common::ast::{CallExpr, Expr, IdedExpr, LiteralValue, SelectExpr};
use cel_core_proto::r#gen::cel::expr::{constant, expr, r#type};
use cel_core_proto::{CheckedExpr, Type};
use prost::Message;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};

// Ordered maps retain CEL's wire format and make persisted bytes/checksums repeatable.
#[derive(Message)]
struct OrderedCheckedExpr {
    #[prost(btree_map = "int64, message", tag = "2")]
    reference_map: BTreeMap<i64, cel_core_proto::r#gen::cel::expr::Reference>,
    #[prost(btree_map = "int64, message", tag = "3")]
    type_map: BTreeMap<i64, Type>,
    #[prost(message, optional, tag = "4")]
    expr: Option<cel_core_proto::Expr>,
    #[prost(message, optional, tag = "5")]
    source_info: Option<OrderedSourceInfo>,
    #[prost(string, tag = "6")]
    expr_version: String,
}

#[derive(Message)]
struct OrderedSourceInfo {
    #[prost(string, tag = "1")]
    syntax_version: String,
    #[prost(string, tag = "2")]
    location: String,
    #[prost(int32, repeated, tag = "3")]
    line_offsets: Vec<i32>,
    #[prost(btree_map = "int64, int32", tag = "4")]
    positions: BTreeMap<i64, i32>,
    #[prost(btree_map = "int64, message", tag = "5")]
    macro_calls: BTreeMap<i64, cel_core_proto::Expr>,
    #[prost(message, repeated, tag = "6")]
    extensions: Vec<cel_core_proto::r#gen::cel::expr::source_info::Extension>,
}

impl From<CheckedExpr> for OrderedCheckedExpr {
    fn from(checked: CheckedExpr) -> Self {
        Self {
            reference_map: checked.reference_map.into_iter().collect(),
            type_map: checked.type_map.into_iter().collect(),
            expr: checked.expr,
            expr_version: checked.expr_version,
            source_info: checked.source_info.map(|source| OrderedSourceInfo {
                syntax_version: source.syntax_version,
                location: source.location,
                line_offsets: source.line_offsets,
                positions: source.positions.into_iter().collect(),
                macro_calls: source.macro_calls.into_iter().collect(),
                extensions: source.extensions,
            }),
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckedExpression {
    pub canonical_expression: String,
    pub referenced_attributes: Vec<String>,
    pub checked_ast: String,
    pub checked_ast_digest: String,
}

pub fn compile(
    expression: &IdedExpr,
    source: &str,
    schemas: &serde_json::Value,
    canonical: String,
) -> Result<CheckedExpression, String> {
    use cel_core::{CelType, Env};
    use cel_core_proto::AstToProto;
    fn schema(value: &serde_json::Value) -> CelType {
        match value["type"].as_str() {
            Some("null") => CelType::Null,
            Some("boolean") => CelType::Bool,
            Some("integer") => CelType::Int,
            Some("number") => CelType::Double,
            Some("string") => CelType::String,
            Some("array") => CelType::list(schema(&value["items"])),
            Some("object") => CelType::map(CelType::String, CelType::Dyn),
            _ => CelType::Dyn,
        }
    }
    // A dynamic root declaration shadows the qualified schema declarations in cel-core.
    let mut environment =
        Env::with_standard_library().with_variable("attributes._random", CelType::Double);
    for (name, value) in schemas.as_object().ok_or("schemas must be an object")? {
        environment.add_variable(format!("attributes.{name}"), schema(value));
    }
    let parsed = cel_core_proto::ParsedExpr {
        expr: Some(to_proto(expression)?),
        source_info: Some(cel_core_proto::SourceInfo::default()),
    };
    let expression =
        cel_core_proto::from_parsed_expr(&parsed).map_err(|error| error.to_string())?;
    let type_info = environment.check(&expression);
    if !type_info.is_ok() {
        return Err(format!(
            "checked compilation failed: {:?}",
            type_info.errors
        ));
    }
    let ast = cel_core::Ast::new_checked(expression, source, type_info);
    let mut checked = ast.to_checked_expr().map_err(|error| error.to_string())?;
    fn normalize(node: &mut cel_core_proto::Expr) {
        if let Some(expr::ExprKind::CallExpr(call)) = &mut node.expr_kind {
            // cel-core-proto emits subtraction's name for unary negation; CEL uses "-_".
            if call.function == "_-_" && call.args.len() == 1 {
                call.function = "-_".to_owned();
            }
            for argument in &mut call.args {
                normalize(argument);
            }
        }
    }
    normalize(
        checked
            .expr
            .as_mut()
            .ok_or("compiled expression is missing its AST")?,
    );
    let mut nodes = 0;
    let mut references = BTreeSet::new();
    convert(
        checked
            .expr
            .as_ref()
            .ok_or("compiled expression is missing its AST")?,
        1,
        &mut nodes,
        &mut references,
    )?;
    let bytes = OrderedCheckedExpr::from(checked).encode_to_vec();
    if bytes.is_empty() || bytes.len() > 65_536 {
        return Err("compiled checked expression exceeds byte limits".to_owned());
    }
    Ok(CheckedExpression {
        canonical_expression: canonical,
        referenced_attributes: references.into_iter().collect(),
        checked_ast: STANDARD.encode(&bytes),
        checked_ast_digest: format!("sha256:{:x}", Sha256::digest(&bytes)),
    })
}

fn to_proto(node: &IdedExpr) -> Result<cel_core_proto::Expr, String> {
    use cel_core_proto::Constant;
    use constant::ConstantKind;
    use expr::ExprKind;
    let kind = match &node.expr {
        Expr::Literal(value) => ExprKind::ConstExpr(Constant {
            constant_kind: Some(match value {
                LiteralValue::Null => ConstantKind::NullValue(0),
                LiteralValue::Boolean(value) => ConstantKind::BoolValue(*value.inner()),
                LiteralValue::Int(value) => ConstantKind::Int64Value(*value.inner()),
                LiteralValue::Double(value) => ConstantKind::DoubleValue(*value.inner()),
                LiteralValue::String(value) => ConstantKind::StringValue(value.inner().to_owned()),
                _ => return Err("unsupported literal for checked compilation".to_owned()),
            }),
        }),
        Expr::Ident(name) if name == "attributes" => {
            ExprKind::IdentExpr(expr::Ident { name: name.clone() })
        }
        Expr::Select(select) if !select.test => ExprKind::SelectExpr(Box::new(expr::Select {
            operand: Some(Box::new(to_proto(&select.operand)?)),
            field: select.field.clone(),
            test_only: false,
        })),
        Expr::Call(call) if call.target.is_none() => ExprKind::CallExpr(Box::new(expr::Call {
            target: None,
            function: if call.func_name == "-_" {
                "_-_".to_owned()
            } else {
                call.func_name.clone()
            },
            args: call.args.iter().map(to_proto).collect::<Result<_, _>>()?,
        })),
        _ => return Err("unsupported AST for checked compilation".to_owned()),
    };
    Ok(cel_core_proto::Expr {
        id: i64::try_from(node.id).map_err(|error| error.to_string())?,
        expr_kind: Some(kind),
    })
}

pub fn decode(stored: &CheckedExpression) -> Result<(IdedExpr, HashMap<i64, Type>), String> {
    if stored.checked_ast.len() > 87_384 {
        return Err("checked expression exceeds the encoded size limit".to_owned());
    }
    let bytes = STANDARD
        .decode(&stored.checked_ast)
        .map_err(|error| error.to_string())?;
    if bytes.is_empty() || bytes.len() > 65_536 {
        return Err("checked expression exceeds the byte limits".to_owned());
    }
    if format!("sha256:{:x}", Sha256::digest(&bytes)) != stored.checked_ast_digest {
        return Err("checked expression digest does not match".to_owned());
    }
    let checked = CheckedExpr::decode(bytes.as_slice()).map_err(|error| error.to_string())?;
    let expression = checked
        .expr
        .ok_or_else(|| "checked expression is missing its AST".to_owned())?;
    let mut references = BTreeSet::new();
    let mut nodes = 0;
    let ast = convert(&expression, 1, &mut nodes, &mut references)?;
    let expected = stored
        .referenced_attributes
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    if expected.len() != stored.referenced_attributes.len() || references != expected {
        return Err("checked expression attribute references do not match".to_owned());
    }
    Ok((ast, checked.type_map))
}

pub fn primitive(kind: &Type) -> Result<super::profile::Kind, String> {
    use super::profile::Kind;
    match kind.type_kind.as_ref() {
        Some(r#type::TypeKind::Null(_)) => Ok(Kind::Null),
        Some(r#type::TypeKind::Primitive(1)) => Ok(Kind::Bool),
        Some(r#type::TypeKind::Primitive(2)) => Ok(Kind::Int),
        Some(r#type::TypeKind::Primitive(4)) => Ok(Kind::Double),
        Some(r#type::TypeKind::Primitive(5)) => Ok(Kind::String),
        Some(r#type::TypeKind::ListType(_)) => Ok(Kind::List),
        Some(r#type::TypeKind::MapType(_)) => Ok(Kind::Map),
        _ => Err("checked expression has an unsupported type".to_owned()),
    }
}

fn convert(
    node: &cel_core_proto::Expr,
    depth: usize,
    nodes: &mut usize,
    references: &mut BTreeSet<String>,
) -> Result<IdedExpr, String> {
    *nodes += 1;
    if depth > 32 || *nodes > 256 {
        return Err("checked expression exceeds AST limits".to_owned());
    }
    let expr = match node.expr_kind.as_ref() {
        Some(expr::ExprKind::ConstExpr(value)) => {
            Expr::Literal(match value.constant_kind.as_ref() {
                Some(constant::ConstantKind::NullValue(_)) => LiteralValue::Null,
                Some(constant::ConstantKind::BoolValue(value)) => {
                    LiteralValue::Boolean((*value).into())
                }
                Some(constant::ConstantKind::Int64Value(value)) => {
                    LiteralValue::Int((*value).into())
                }
                Some(constant::ConstantKind::DoubleValue(value)) => {
                    LiteralValue::Double((*value).into())
                }
                Some(constant::ConstantKind::StringValue(value)) => {
                    LiteralValue::String(value.clone().into())
                }
                _ => return Err("unsupported checked literal".to_owned()),
            })
        }
        Some(expr::ExprKind::IdentExpr(value)) if value.name == "attributes" => {
            Expr::Ident(value.name.clone())
        }
        Some(expr::ExprKind::SelectExpr(value)) if !value.test_only => {
            let operand = value
                .operand
                .as_deref()
                .ok_or_else(|| "checked selection is missing its operand".to_owned())?;
            if !matches!(&operand.expr_kind, Some(expr::ExprKind::IdentExpr(ident)) if ident.name == "attributes")
            {
                return Err("only direct attribute selections are supported".to_owned());
            }
            references.insert(value.field.clone());
            Expr::Select(SelectExpr {
                operand: Box::new(convert(operand, depth + 1, nodes, references)?),
                field: value.field.clone(),
                test: false,
            })
        }
        Some(expr::ExprKind::CallExpr(value)) if value.target.is_none() => Expr::Call(CallExpr {
            func_name: value.function.clone(),
            target: None,
            args: value
                .args
                .iter()
                .map(|argument| convert(argument, depth + 1, nodes, references))
                .collect::<Result<_, _>>()?,
        }),
        _ => return Err("unsupported checked expression form".to_owned()),
    };
    let id = u64::try_from(node.id).map_err(|error| error.to_string())?;
    Ok(IdedExpr { id, expr })
}

#[cfg(test)]
mod tests {
    use super::{compile, decode, primitive};
    use crate::profile::Kind;
    use cel::common::ast::Expr;
    use serde_json::json;

    fn compiled(source: &str, schemas: &serde_json::Value) -> super::CheckedExpression {
        let ast = cel::parser::PrattParser::default().parse(source).unwrap();
        compile(&ast, source, schemas, source.to_owned()).unwrap()
    }

    #[test]
    fn compiled_qualified_attributes_retain_the_declared_type() {
        let stored = compiled("attributes.label", &json!({"label": {"type": "string"}}));
        let (ast, types) = decode(&stored).unwrap();
        let id = i64::try_from(ast.id).unwrap();
        assert_eq!(primitive(types.get(&id).unwrap()).unwrap(), Kind::String);
    }

    #[test]
    fn compiled_negation_uses_the_standard_cel_function_name() {
        let stored = compiled("-attributes.level", &json!({"level": {"type": "integer"}}));
        let (ast, _) = decode(&stored).unwrap();
        assert!(matches!(ast.expr, Expr::Call(call) if call.func_name == "-_"));
    }

    #[test]
    fn checked_bytes_are_repeatable_for_the_same_source_and_schema() {
        let schemas = json!({"level": {"type": "integer"}});
        let source = "double(attributes.level) + 2.5";
        let first = compiled(source, &schemas);
        for _ in 0..16 {
            let repeated = compiled(source, &schemas);
            assert_eq!(first.checked_ast, repeated.checked_ast);
            assert_eq!(first.checked_ast_digest, repeated.checked_ast_digest);
        }
    }
}
