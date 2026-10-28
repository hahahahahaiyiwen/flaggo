use flaggo_contract::ContractError;
use flaggo_expression::{CheckedDecisionExecutable, CheckedExecutableRule, CheckedExpression};
use serde::{
    Deserialize, Deserializer,
    de::{self, MapAccess, Visitor},
};
use std::fmt;

// JsonSerializer permits repeated checked-DTO members, but validates each occurrence.
// Collapsing into a JSON map first would hide an invalid earlier occurrence.
macro_rules! checked_object {
    ($wire:ident, $model:ty, {$($field:ident: $kind:ty => $name:literal),+}, $build:expr) => {
        struct $wire($model);
        impl<'de> Deserialize<'de> for $wire {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct Object;
                impl<'de> Visitor<'de> for Object {
                    type Value = $wire;
                    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                        formatter.write_str("a checked executable object")
                    }
                    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                        $(let mut $field: Option<$kind> = None;)+
                        while let Some(key) = map.next_key::<String>()? {
                            match key.as_str() {
                                $($name => $field = Some(map.next_value()?),)+
                                _ => return Err(de::Error::unknown_field(&key, &[$($name),+])),
                            }
                        }
                        ($build)($($field),+).map($wire)
                    }
                }
                deserializer.deserialize_map(Object)
            }
        }
    };
}

fn required<T, E: de::Error>(value: Option<T>, field: &'static str) -> Result<T, E> {
    value.ok_or_else(|| de::Error::missing_field(field))
}

checked_object!(Executable, CheckedDecisionExecutable, {
    rules: Vec<Option<Rule>> => "rules"
}, |rules: Option<Vec<Option<Rule>>>| {
    Ok(CheckedDecisionExecutable {
        rules: required(rules, "rules")?.into_iter().map(|rule| rule.map(|rule| rule.0)).collect(),
    })
});

checked_object!(Rule, CheckedExecutableRule, {
    name: String => "name",
    predicate: Expression => "predicate",
    result: Option<Expression> => "result"
}, |name: Option<String>, predicate: Option<Expression>, result: Option<Option<Expression>>| {
    Ok(CheckedExecutableRule {
        name: required(name, "name")?,
        predicate: required(predicate, "predicate")?.0,
        result: result.flatten().map(|value| value.0),
    })
});

checked_object!(Expression, CheckedExpression, {
    canonical: String => "canonicalExpression",
    attributes: Vec<String> => "referencedAttributes",
    ast: String => "checkedAst",
    digest: String => "checkedAstDigest"
}, |canonical: Option<String>, attributes: Option<Vec<String>>, ast: Option<String>, digest: Option<String>| {
    Ok(CheckedExpression {
        canonical_expression: required(canonical, "canonicalExpression")?,
        referenced_attributes: required(attributes, "referencedAttributes")?,
        checked_ast: required(ast, "checkedAst")?,
        checked_ast_digest: required(digest, "checkedAstDigest")?,
    })
});

pub(crate) fn read(bytes: &[u8]) -> Result<Option<CheckedDecisionExecutable>, ContractError> {
    Ok(serde_json::from_slice::<Option<Executable>>(bytes)?.map(|value| value.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"{"rules":[{"name":"r","predicate":{"canonicalExpression":"true","referencedAttributes":[],"checkedAst":"bytes","checkedAstDigest":"digest"}}]}"#;

    #[test]
    fn valid_repeated_members_use_the_last_value_without_strict_token_limits() {
        let repeated = format!(r#"{{"rules":[],{}"#, &VALID[1..]);
        let checked = read(repeated.as_bytes()).unwrap().unwrap();
        assert_eq!(checked.rules.len(), 1);
        assert_eq!(checked.rules[0].as_ref().unwrap().name, "r");
        let long = VALID.replace("\"true\"", &format!("\"{}\"", "x".repeat(16385)));
        assert!(read(long.as_bytes()).is_ok());
    }

    #[test]
    fn invalid_earlier_occurrences_are_not_hidden_by_later_valid_values() {
        for invalid in [
            format!(r#"{{"rules":null,{}"#, &VALID[1..]),
            VALID.replace("\"predicate\":{", "\"predicate\":null,\"predicate\":{"),
            VALID.replace("\"checkedAst\":", "\"checkedAst\":false,\"checkedAst\":"),
        ] {
            assert!(matches!(
                read(invalid.as_bytes()),
                Err(ContractError::Json(_))
            ));
        }
        assert!(read(b"null").unwrap().is_none());
    }
}
