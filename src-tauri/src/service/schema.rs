// SPDX-License-Identifier: GPL-3.0-or-later
// MCP tool schemas and strict JSON-schema argument validation.
use schemars::{JsonSchema, generate::SchemaSettings};
use serde_json::Value;

pub(super) fn derived<T: JsonSchema>() -> Value {
    use schemars::transform::{RecursiveTransform, Transform};
    let mut schema = schema_for::<T>(SchemaSettings::default());
    RecursiveTransform(|s: &mut schemars::Schema| {
        s.remove("default");
        s.remove("format");
        s.remove("description");
    })
    .transform(&mut schema);
    schema.to_value()
}

pub(super) fn result<T: JsonSchema>() -> Value {
    schema_for::<T>(SchemaSettings::default().for_serialize()).to_value()
}

fn schema_for<T: JsonSchema>(mut settings: SchemaSettings) -> schemars::Schema {
    settings.inline_subschemas = true;
    settings.into_generator().subschema_for::<T>()
}

pub fn system_schema() -> Value {
    derived::<super::System>()
}
pub fn bench_schema() -> Value {
    derived::<super::bench::Bench>()
}

pub fn validate_schema(value: &Value, schema: &Value, path: &str) -> Result<(), String> {
    if schema == &Value::Bool(true) {
        return Ok(());
    }
    if let Some(choices) = schema["oneOf"]
        .as_array()
        .or_else(|| schema["anyOf"].as_array())
    {
        let results: Vec<_> = choices
            .iter()
            .map(|s| validate_schema(value, s, path))
            .collect();
        let matching = results.iter().filter(|r| r.is_ok()).count();
        return if matching == 1 || (schema.get("anyOf").is_some() && matching > 0) {
            Ok(())
        } else if matching == 0 {
            Err(format!(
                "Invalid definition at {path}: {}",
                results
                    .into_iter()
                    .filter_map(Result::err)
                    .collect::<Vec<_>>()
                    .join("; ")
            ))
        } else {
            Err(format!("Ambiguous definition at {path}"))
        };
    }
    let matches = |kind: &str| match kind {
        "object" => value.is_object(),
        "array" => value.is_array(),
        "string" => value.is_string(),
        "number" => value.as_f64().is_some_and(|n| n.is_finite()),
        "integer" => value.as_u64().is_some(),
        "boolean" => value.is_boolean(),
        "null" => value.is_null(),
        _ => false,
    };
    let valid = if let Some(k) = schema["type"].as_str() {
        matches(k)
    } else if let Some(kinds) = schema["type"].as_array() {
        kinds.iter().any(|k| matches(k.as_str().unwrap_or("")))
    } else {
        true
    };
    if !valid {
        return Err(format!("Invalid type at {path}"));
    }
    if let Some(expected) = schema.get("const")
        && value != expected
    {
        return Err(format!("Unknown value at {path}"));
    }
    if value.is_null() {
        return Ok(());
    }
    if let Some(n) = value.as_f64() {
        for (bound, inclusive, lower) in [
            ("minimum", true, true),
            ("maximum", true, false),
            ("exclusiveMinimum", false, true),
            ("exclusiveMaximum", false, false),
        ] {
            if let Some(b) = schema[bound].as_f64()
                && if lower {
                    n < b || (!inclusive && n == b)
                } else {
                    n > b || (!inclusive && n == b)
                }
            {
                return Err(format!("Value outside range at {path}"));
            }
        }
    }
    if let Some(allowed) = schema["enum"].as_array()
        && !allowed.contains(value)
    {
        return Err(format!("Unknown value at {path}"));
    }
    if let Some(text) = value.as_str()
        && text.len() > 8_000_000
    {
        return Err(format!("Text too large at {path}"));
    }
    if let Some(items) = value.as_array() {
        let min = schema["minItems"].as_u64().unwrap_or(0) as usize;
        let max = schema["maxItems"].as_u64().unwrap_or(100_000) as usize;
        if items.len() < min || items.len() > max {
            return Err(format!("Invalid array length at {path}"));
        }
        for (i, v) in items.iter().enumerate() {
            validate_schema(v, &schema["items"], &format!("{path}[{i}]"))?;
        }
    }
    if let Some(object) = value.as_object() {
        if let Some(required) = schema["required"].as_array() {
            for key in required {
                let key = key.as_str().unwrap();
                if !object.contains_key(key) {
                    return Err(format!("Missing {path}.{key}"));
                }
            }
        }
        for (key, v) in object {
            let child = schema["properties"].get(key).or_else(|| {
                schema.get("additionalProperties").filter(|extra| **extra != false)
            }).ok_or_else(|| format!("Unknown argument {path}.{key}"))?;
            validate_schema(v, child, &format!("{path}.{key}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    use serde_json::json;

    fn sort(items: &mut [Value]) {
        items.sort_by_cached_key(Value::to_string);
    }

    // Compare assertion keywords. Expand nullable types and alternatives grouped
    // by a required discriminator into the same disjoint oneOf representation.
    fn canonical(mut schema: Value) -> Value {
        let obj = schema.as_object_mut().unwrap();
        for annotation in ["title", "description", "format", "default"] {
            obj.remove(annotation);
        }
        if let Some(constant) = obj.remove("const") {
            obj.insert("enum".into(), json!([constant]));
        }
        if let Some(types) = obj.get("type").and_then(Value::as_array).cloned() {
            let allows_null = obj
                .get("enum")
                .and_then(Value::as_array)
                .is_none_or(|values| values.contains(&Value::Null));
            let branches: Vec<_> = types
                .into_iter()
                .filter(|t| t != "null" || allows_null)
                .map(|t| {
                    if t == "null" {
                        return json!({"type":"null"});
                    }
                    let mut branch = schema.clone();
                    branch["type"] = t;
                    if let Some(values) = branch.get_mut("enum").and_then(Value::as_array_mut) {
                        values.retain(|v| !v.is_null());
                    }
                    branch
                })
                .collect();
            return canonical(json!({"oneOf":branches}));
        }
        if let Some(choices) = obj.remove("anyOf") {
            // The only anyOf emitted by these types is Option<tagged enum>.
            let choices = choices.as_array().unwrap();
            assert_eq!(choices.len(), 2);
            assert_eq!(choices[1], json!({"type":"null"}));
            assert!(
                choices[0]["oneOf"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|s| s["type"] == "object")
            );
            obj.insert("oneOf".into(), Value::Array(choices.clone()));
        }
        if let Some(choices) = obj.remove("oneOf") {
            let mut branches = Vec::new();
            for branch in choices.as_array().unwrap() {
                let normalized = canonical(branch.clone());
                if let Some(nested) = normalized["oneOf"].as_array()
                    && normalized.as_object().unwrap().len() == 1
                {
                    branches.extend(nested.iter().cloned());
                } else if let Some(tags) = normalized
                    .pointer("/properties/kind/enum")
                    .and_then(Value::as_array)
                    && tags.len() > 1
                    && normalized["required"]
                        .as_array()
                        .unwrap()
                        .contains(&json!("kind"))
                {
                    for tag in tags {
                        let mut single = normalized.clone();
                        single["properties"]["kind"]["enum"] = json!([tag]);
                        branches.push(single);
                    }
                } else {
                    branches.push(normalized);
                }
            }
            sort(&mut branches);
            obj.insert("oneOf".into(), Value::Array(branches));
        }
        if let Some(properties) = obj.get_mut("properties").and_then(Value::as_object_mut) {
            for child in properties.values_mut() {
                *child = canonical(child.take());
            }
            if properties.is_empty() {
                obj.remove("properties");
            }
        }
        if let Some(items) = obj.get_mut("items") {
            *items = canonical(items.take());
        }
        if let Some(required) = obj.get_mut("required").and_then(Value::as_array_mut) {
            sort(required);
            if required.is_empty() {
                obj.remove("required");
            }
        }
        if let Some(values) = obj.get_mut("enum").and_then(Value::as_array_mut) {
            sort(values);
            if values.iter().all(|v| v.as_u64().is_some()) && obj.get("minimum") == Some(&json!(0))
            {
                obj.remove("minimum");
            }
        }
        schema
    }

    #[test]
    fn tools_list_matches_published_contract() {
        let expected: Vec<Value> =
            serde_json::from_str(include_str!("../../tests/fixtures/tools_list_before.json"))
                .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut workspace = super::super::Workspace::open(dir.path().join("system.json")).unwrap();
        let response =
            super::super::rpc(json!({"id":1,"method":"tools/list"}), &mut workspace).unwrap();
        let listed = &response["result"]["tools"];
        let actual = listed.as_array().unwrap();
        assert_eq!(actual.len(), expected.len());
        for (tool, old) in actual.iter().zip(expected) {
            assert_eq!(tool["name"], old["name"]);
            assert_eq!(tool["description"], old["description"]);
            assert_eq!(
                canonical(tool["inputSchema"].clone()),
                canonical(old["inputSchema"].clone()),
                "{}",
                tool["name"]
            );
        }
    }
}
