use cel::{Context, Env};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{self, BufRead};
use std::sync::Arc;

mod checked;
mod domain;
mod profile;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    id: String,
    expression: String,
    schemas: Value,
    result_type: String,
    attributes: Value,
    compile_only: bool,
    #[serde(default)]
    checked: Option<checked::CheckedExpression>,
    #[serde(default)]
    emit_checked: bool,
}

fn cel_input(value: &Value) -> Result<cel::Value, io::Error> {
    match value {
        Value::Null => Ok(cel::Value::Null),
        Value::Bool(value) => Ok(cel::Value::Bool(*value)),
        Value::String(value) => Ok(cel::Value::String(value.clone().into())),
        Value::Number(value) => {
            if let Some(integer) = value.as_i64() {
                return Ok(cel::Value::Int(integer));
            }
            let number = value
                .as_f64()
                .filter(|number| number.is_finite())
                .ok_or_else(|| {
                    io::Error::other("input number is outside the Flaggo JSON profile")
                })?;
            Ok(cel::Value::Float(number))
        }
        Value::Array(values) => Ok(cel::Value::List(
            values
                .iter()
                .map(cel_input)
                .collect::<Result<Vec<_>, _>>()?
                .into(),
        )),
        Value::Object(values) => {
            let entries = values
                .iter()
                .map(|(name, value)| Ok((name.clone(), cel_input(value)?)))
                .collect::<Result<HashMap<_, _>, io::Error>>()?;
            Ok(cel::Value::Map(entries.into()))
        }
    }
}

fn evaluate(request: Request, environment: &Arc<Env>) -> Result<Value, Box<dyn std::error::Error>> {
    // This deliberately measures the upstream library, not an approved Flaggo compiler.
    let _ = (&request.schemas, &request.result_type);
    let program = match environment.compile(&request.expression) {
        Ok(program) => program,
        Err(error) => {
            return Ok(json!({
                "id": request.id, "status": "compile-error", "message": error.to_string()
            }));
        }
    };
    if request.compile_only {
        return Ok(json!({"id": request.id, "status": "compiled"}));
    }
    let mut context = Context::with_env(Arc::clone(environment));
    context.add_variable_from_value("attributes", cel_input(&request.attributes)?);
    match program.execute(&context) {
        Ok(value) => {
            if matches!(&value, cel::Value::Float(number) if !number.is_finite()) {
                return Ok(json!({
                    "id": request.id,
                    "status": "evaluation-error",
                    "message": "non-finite result is outside the Flaggo JSON profile"
                }));
            }
            let result = value
                .json()
                .map_err(|error| io::Error::other(error.to_string()))?;
            Ok(json!({"id": request.id, "status": "value", "value": result}))
        }
        Err(error) => Ok(json!({
            "id": request.id, "status": "evaluation-error", "message": error.to_string()
        })),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let environment = Arc::new(Env::default().with_optional_support(false).with_stdlib());
    let profile = std::env::args().any(|argument| argument == "--profile");
    for line in io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line?)?;
        if request.get("domainModel").is_some() {
            println!("{}", domain::evaluate(serde_json::from_value(request)?)?);
            continue;
        }
        if let Some(value) = request.get("canonicalJson") {
            println!(
                "{}",
                json!({
                    "id": request["id"], "status": "value", "value": serde_jcs::to_string(value)?
                })
            );
            continue;
        }
        let request = serde_json::from_value(request)?;
        let response = if profile {
            profile::evaluate(request, &environment)?
        } else {
            evaluate(request, &environment)?
        };
        println!("{response}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::cel_input;
    use serde_json::json;

    #[test]
    fn json_numbers_follow_the_existing_signed_int_then_double_boundary() {
        assert_eq!(cel_input(&json!(10)).unwrap(), cel::Value::Int(10));
        assert_eq!(
            cel_input(&json!(i64::MAX)).unwrap(),
            cel::Value::Int(i64::MAX)
        );
        assert_eq!(
            cel_input(&json!(u64::MAX)).unwrap(),
            cel::Value::Float(u64::MAX as f64)
        );
        assert_eq!(cel_input(&json!(1.5)).unwrap(), cel::Value::Float(1.5));
    }
}
