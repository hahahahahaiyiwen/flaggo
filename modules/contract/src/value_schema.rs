use crate::JsonValue as Value;
use crate::strict_json::{MAXIMUM_DEPTH, MAXIMUM_STRING_BYTES};
use crate::validation::{MAXIMUM_COLLECTION_SIZE, error, escape_pointer};
use crate::{ContractError, ValidationIssue, ValueSchema};
use std::collections::HashSet;

pub fn validate_schema(schema: &ValueSchema, path: &str, issues: &mut Vec<ValidationIssue>) {
    schema_at_depth(schema, path, issues, 0);
}

fn schema_at_depth(
    schema: &ValueSchema,
    path: &str,
    issues: &mut Vec<ValidationIssue>,
    depth: usize,
) {
    if depth > MAXIMUM_DEPTH {
        error(
            issues,
            "schema-too-deep",
            path,
            format!("Value schema exceeds {MAXIMUM_DEPTH} levels."),
        );
        return;
    }
    match schema.kind.as_str() {
        "null" | "boolean" => {
            reject_numeric(schema, path, issues);
            reject_string(schema, path, issues);
            reject_collections(schema, path, issues);
        }
        "integer" | "number" => {
            if schema.kind == "integer" {
                for (name, value) in [
                    ("minimum", schema.minimum),
                    ("maximum", schema.maximum),
                    ("exclusiveMinimum", schema.exclusive_minimum),
                    ("exclusiveMaximum", schema.exclusive_maximum),
                    ("multipleOf", schema.multiple_of),
                ] {
                    if value.is_some_and(|value| value != value.trunc()) {
                        error(
                            issues,
                            "non-integer-schema-keyword",
                            &format!("{path}/{name}"),
                            format!("{name} must be an integer for an integer schema."),
                        );
                    }
                }
            }
            if schema
                .minimum
                .or(schema.exclusive_minimum)
                .zip(schema.maximum.or(schema.exclusive_maximum))
                .is_some_and(|(lower, upper)| lower > upper)
            {
                error(
                    issues,
                    "invalid-schema-range",
                    path,
                    "Lower numeric bound must not exceed upper bound.",
                );
            }
            if schema.multiple_of.is_some_and(|value| value <= 0.0) {
                error(
                    issues,
                    "invalid-multiple-of",
                    &format!("{path}/multipleOf"),
                    "multipleOf must be positive.",
                );
            }
            reject_string(schema, path, issues);
            reject_collections(schema, path, issues);
        }
        "string" => {
            validate_range(
                schema.min_length,
                schema.max_length,
                MAXIMUM_STRING_BYTES,
                path,
                "length",
                issues,
            );
            reject_numeric(schema, path, issues);
            reject_collections(schema, path, issues);
        }
        "array" => {
            if let Some(items) = &schema.items {
                schema_at_depth(items, &format!("{path}/items"), issues, depth + 1);
            } else {
                error(
                    issues,
                    "missing-items",
                    &format!("{path}/items"),
                    "Array schemas require items.",
                );
            }
            validate_range(
                schema.min_items,
                schema.max_items,
                MAXIMUM_COLLECTION_SIZE,
                path,
                "items",
                issues,
            );
            reject_numeric(schema, path, issues);
            reject_object(schema, path, issues);
        }
        "object" => {
            if schema.properties.is_none() || schema.additional_properties != Some(false) {
                error(
                    issues,
                    "open-object-schema",
                    path,
                    "Object schemas require properties and additionalProperties: false.",
                );
            } else if let Some(properties) = &schema.properties {
                if properties.len() > MAXIMUM_COLLECTION_SIZE {
                    error(
                        issues,
                        "too-many-schema-properties",
                        &format!("{path}/properties"),
                        "Object schema has too many properties.",
                    );
                }
                for (name, property) in properties {
                    let property_path = format!("{path}/properties/{}", escape_pointer(name));
                    if let Some(property) = property {
                        schema_at_depth(property, &property_path, issues, depth + 1);
                    } else {
                        error(
                            issues,
                            "null-property-schema",
                            &property_path,
                            "Property schemas must not be null.",
                        );
                    }
                }
            }
            let required = schema.required.as_deref().unwrap_or_default();
            if required.iter().collect::<HashSet<_>>().len() != required.len() {
                error(
                    issues,
                    "duplicate-required-property",
                    &format!("{path}/required"),
                    "Required property names must be unique.",
                );
            }
            if let Some(properties) = &schema.properties {
                for name in required {
                    match name.as_deref() {
                        None | Some("") => error(
                            issues,
                            "invalid-required-property",
                            &format!("{path}/required"),
                            "Required property names must not be null or empty.",
                        ),
                        Some(name) if !properties.contains_key(name) => error(
                            issues,
                            "unknown-required-property",
                            &format!("{path}/required"),
                            format!("Required property '{name}' is not declared."),
                        ),
                        _ => {}
                    }
                }
            }
            validate_range(
                schema.min_properties,
                schema.max_properties,
                MAXIMUM_COLLECTION_SIZE,
                path,
                "properties",
                issues,
            );
            reject_numeric(schema, path, issues);
            reject_array(schema, path, issues);
        }
        kind => error(
            issues,
            "unsupported-value-type",
            &format!("{path}/type"),
            format!("Value schema type '{kind}' is not supported."),
        ),
    }
}

pub fn validate_value(
    schema: &ValueSchema,
    value: &Value,
    path: &str,
    issues: &mut Vec<ValidationIssue>,
) -> Result<(), ContractError> {
    value_at_depth(schema, value, path, issues, 0)
}

fn value_at_depth(
    schema: &ValueSchema,
    value: &Value,
    path: &str,
    issues: &mut Vec<ValidationIssue>,
    depth: usize,
) -> Result<(), ContractError> {
    if depth > MAXIMUM_DEPTH {
        error(
            issues,
            "value-too-deep",
            path,
            format!("Value exceeds {MAXIMUM_DEPTH} levels."),
        );
        return Ok(());
    }
    match schema.kind.as_str() {
        "null" if !value.is_null() => type_error(path, "null", issues),
        "boolean" if !value.is_boolean() => type_error(path, "boolean", issues),
        "null" | "boolean" => {}
        "integer" => {
            if let Some(value) = value.as_i64() {
                validate_number(value as f64, schema, path, issues);
            } else {
                type_error(path, "signed 64-bit integer", issues);
            }
        }
        "number" => {
            if let Some(value) = value.as_f64().filter(|value| value.is_finite()) {
                validate_number(value, schema, path, issues);
            } else {
                type_error(path, "finite number", issues);
            }
        }
        "string" => {
            if let Some(text) = value.as_str() {
                let length = text.chars().count();
                if text.len() > MAXIMUM_STRING_BYTES
                    || below(length, schema.min_length)
                    || above(length, schema.max_length)
                {
                    error(
                        issues,
                        "string-out-of-range",
                        path,
                        "String does not satisfy the configured length bounds.",
                    );
                }
            } else {
                type_error(path, "string", issues);
            }
        }
        "array" => {
            if let Some(items) = value.as_array() {
                if items.len() > MAXIMUM_COLLECTION_SIZE
                    || below(items.len(), schema.min_items)
                    || above(items.len(), schema.max_items)
                {
                    error(
                        issues,
                        "array-out-of-range",
                        path,
                        "Array does not satisfy the configured item bounds.",
                    );
                }
                if schema.unique_items == Some(true) {
                    let mut unique = HashSet::new();
                    for item in items {
                        if !unique.insert(serde_jcs::to_string(item)?) {
                            error(
                                issues,
                                "array-items-not-unique",
                                path,
                                "Array items must be unique.",
                            );
                            break;
                        }
                    }
                }
                if let Some(schema) = &schema.items {
                    for (index, item) in items.iter().enumerate() {
                        value_at_depth(
                            schema,
                            item,
                            &format!("{path}/{index}"),
                            issues,
                            depth + 1,
                        )?;
                    }
                }
            } else {
                type_error(path, "array", issues);
            }
        }
        "object" => {
            if let Some(properties) = value.as_object() {
                if properties.len() > MAXIMUM_COLLECTION_SIZE
                    || below(properties.len(), schema.min_properties)
                    || above(properties.len(), schema.max_properties)
                {
                    error(
                        issues,
                        "object-out-of-range",
                        path,
                        "Object does not satisfy the configured property bounds.",
                    );
                }
                for name in schema.required.as_deref().unwrap_or_default() {
                    let name = name.as_deref().ok_or_else(|| {
                        ContractError::InvalidModel(
                            "null required property during value validation".into(),
                        )
                    })?;
                    if !properties.contains_key(name) {
                        error(
                            issues,
                            "required-property-missing",
                            &format!("{path}/{}", escape_pointer(name)),
                            format!("Required property '{name}' is missing."),
                        );
                    }
                }
                for (name, value) in properties {
                    let property_path = format!("{path}/{}", escape_pointer(name));
                    if let Some(schema) = schema
                        .properties
                        .as_ref()
                        .and_then(|properties| properties.get(name))
                    {
                        let schema = schema.as_ref().ok_or_else(|| {
                            ContractError::InvalidModel(
                                "null property schema during value validation".into(),
                            )
                        })?;
                        value_at_depth(schema, value, &property_path, issues, depth + 1)?;
                    } else {
                        error(
                            issues,
                            "additional-property",
                            &property_path,
                            format!("Property '{name}' is not declared."),
                        );
                    }
                }
            } else {
                type_error(path, "object", issues);
            }
        }
        kind => type_error(path, kind, issues),
    }
    Ok(())
}

fn below(value: usize, bound: Option<i32>) -> bool {
    bound.is_some_and(|bound| (value as i64) < i64::from(bound))
}

fn above(value: usize, bound: Option<i32>) -> bool {
    bound.is_some_and(|bound| (value as i64) > i64::from(bound))
}

fn validate_range(
    minimum: Option<i32>,
    maximum: Option<i32>,
    limit: usize,
    path: &str,
    noun: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    if minimum.is_some_and(|value| value < 0 || value as usize > limit)
        || maximum.is_some_and(|value| value < 0 || value as usize > limit)
        || minimum.zip(maximum).is_some_and(|(min, max)| min > max)
    {
        error(
            issues,
            "invalid-schema-range",
            path,
            format!("Invalid {noun} bounds; values must be between 0 and {limit}."),
        );
    }
}

fn validate_number(
    value: f64,
    schema: &ValueSchema,
    path: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    if schema.minimum.is_some_and(|bound| value < bound)
        || schema.maximum.is_some_and(|bound| value > bound)
        || schema.exclusive_minimum.is_some_and(|bound| value <= bound)
        || schema.exclusive_maximum.is_some_and(|bound| value >= bound)
    {
        error(
            issues,
            "number-out-of-range",
            path,
            "Number does not satisfy the configured bounds.",
        );
    }
    if let Some(multiple) = schema.multiple_of {
        let quotient = value / multiple;
        if (quotient - quotient.round_ties_even()).abs() > 1e-10 {
            error(
                issues,
                "number-not-multiple",
                path,
                format!("Number must be a multiple of {}.", dotnet_number(multiple)),
            );
        }
    }
}

fn dotnet_number(value: f64) -> String {
    let scientific = format!("{value:e}");
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("Rust scientific float format includes an exponent");
    let exponent: i32 = exponent
        .parse()
        .expect("Rust scientific float exponent is an integer");
    if !(-4..17).contains(&exponent) {
        format!("{mantissa}E{exponent:+03}")
    } else {
        value.to_string()
    }
}

fn type_error(path: &str, expected: &str, issues: &mut Vec<ValidationIssue>) {
    error(
        issues,
        "invalid-value-type",
        path,
        format!("Value must be a {expected}."),
    );
}

fn reject_numeric(schema: &ValueSchema, path: &str, issues: &mut Vec<ValidationIssue>) {
    if schema.minimum.is_some()
        || schema.maximum.is_some()
        || schema.exclusive_minimum.is_some()
        || schema.exclusive_maximum.is_some()
        || schema.multiple_of.is_some()
    {
        error(
            issues,
            "inapplicable-schema-keyword",
            path,
            format!("Numeric keywords are not valid for type '{}'.", schema.kind),
        );
    }
}

fn reject_string(schema: &ValueSchema, path: &str, issues: &mut Vec<ValidationIssue>) {
    if schema.min_length.is_some() || schema.max_length.is_some() {
        error(
            issues,
            "inapplicable-schema-keyword",
            path,
            format!("String keywords are not valid for type '{}'.", schema.kind),
        );
    }
}

fn reject_array(schema: &ValueSchema, path: &str, issues: &mut Vec<ValidationIssue>) {
    if schema.items.is_some()
        || schema.min_items.is_some()
        || schema.max_items.is_some()
        || schema.unique_items.is_some()
    {
        error(
            issues,
            "inapplicable-schema-keyword",
            path,
            format!("Array keywords are not valid for type '{}'.", schema.kind),
        );
    }
}

fn reject_object(schema: &ValueSchema, path: &str, issues: &mut Vec<ValidationIssue>) {
    if schema.properties.is_some()
        || schema.required.is_some()
        || schema.additional_properties.is_some()
        || schema.min_properties.is_some()
        || schema.max_properties.is_some()
    {
        error(
            issues,
            "inapplicable-schema-keyword",
            path,
            format!("Object keywords are not valid for type '{}'.", schema.kind),
        );
    }
}

fn reject_collections(schema: &ValueSchema, path: &str, issues: &mut Vec<ValidationIssue>) {
    reject_array(schema, path, issues);
    reject_object(schema, path, issues);
}
