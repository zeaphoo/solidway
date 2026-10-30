//! JSON Schema 机械归一化：只处理 root union / object 形状，不含协议语义。

use std::collections::{BTreeSet, HashSet, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};

use serde_json::{Map, Value, json};

use super::json::resolve_local;

pub(crate) fn normalize_object_schema(schema: Option<&Value>) -> Value {
    let Some(schema) = schema else {
        return empty_object_schema();
    };
    let Some(object) = schema.as_object() else {
        return empty_object_schema();
    };
    let original = Value::Object(object.clone());
    let mut seen = HashSet::new();
    let mut object = match expand_refs(&original, &original, &mut seen, 0) {
        Value::Object(object) => object,
        _ => return empty_object_schema(),
    };
    flatten_root_unions(&mut object);

    if object.get("type").and_then(Value::as_str) != Some("object") {
        return empty_object_schema();
    }
    if !object.get("properties").is_some_and(Value::is_object) {
        object.insert("properties".to_string(), json!({}));
    }
    Value::Object(object)
}

pub(crate) fn custom_tool_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "input": {
                "type": "string",
                "description": "The raw input for this tool, passed through verbatim."
            }
        },
        "required": ["input"]
    })
}

pub(crate) fn flatten_namespace_name(namespace: &str, name: &str) -> String {
    let full = format!("{namespace}__{name}");
    if full.chars().count() <= 64 {
        return full;
    }
    let mut hasher = DefaultHasher::new();
    full.hash(&mut hasher);
    let suffix = format!("__{:016x}", hasher.finish());
    let prefix_len = 64usize.saturating_sub(suffix.chars().count());
    let prefix: String = full.chars().take(prefix_len).collect();
    format!("{prefix}{suffix}")
}

fn empty_object_schema() -> Value {
    json!({"type": "object", "properties": {}})
}

fn expand_refs(value: &Value, root: &Value, seen: &mut HashSet<String>, depth: usize) -> Value {
    if depth > 64 {
        return value.clone();
    }
    match value {
        Value::Object(object) => {
            if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
                if seen.contains(reference) {
                    return json!({});
                }
                let Some(resolved) = resolve_local(root, reference) else {
                    return json!({});
                };
                seen.insert(reference.to_string());
                let mut merged = match expand_refs(&resolved, root, seen, depth + 1) {
                    Value::Object(object) => object,
                    other => return other,
                };
                seen.remove(reference);
                for (key, value) in object {
                    if key != "$ref" {
                        merged.insert(key.clone(), expand_refs(value, root, seen, depth + 1));
                    }
                }
                return Value::Object(merged);
            }
            let mut expanded = Map::new();
            for (key, value) in object {
                if key == "$defs" || key == "definitions" {
                    expanded.insert(key.clone(), value.clone());
                } else {
                    expanded.insert(key.clone(), expand_refs(value, root, seen, depth + 1));
                }
            }
            Value::Object(expanded)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| expand_refs(item, root, seen, depth + 1))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn flatten_root_unions(object: &mut Map<String, Value>) {
    let root_has_additional = object.contains_key("additionalProperties");
    let mut additional_properties = object.get("additionalProperties").cloned();
    let mut properties = object
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut intersection: Option<BTreeSet<String>> = None;
    let mut all_of_required = BTreeSet::new();

    for keyword in ["allOf", "anyOf", "oneOf"] {
        let Some(branches) = object
            .remove(keyword)
            .and_then(|value| value.as_array().cloned())
        else {
            continue;
        };
        for branch in branches {
            let Some(branch) = branch.as_object() else {
                continue;
            };
            if let Some(branch_properties) = branch.get("properties").and_then(Value::as_object) {
                for (name, property) in branch_properties {
                    properties
                        .entry(name.clone())
                        .or_insert_with(|| property.clone());
                }
            }
            if !root_has_additional {
                if keyword == "allOf" {
                    if let Some(value) = branch.get("additionalProperties") {
                        additional_properties = Some(value.clone());
                    }
                } else if additional_properties.is_none() {
                    additional_properties = branch.get("additionalProperties").cloned();
                } else if additional_properties.as_ref() != branch.get("additionalProperties") {
                    additional_properties = None;
                }
            }
            let branch_required: BTreeSet<String> = branch
                .get("required")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(ToString::to_string)
                        .collect()
                })
                .unwrap_or_default();
            if keyword == "allOf" {
                all_of_required.extend(branch_required);
            } else {
                intersection = Some(match intersection {
                    None => branch_required,
                    Some(previous) => previous.intersection(&branch_required).cloned().collect(),
                });
            }
        }
    }

    if !properties.is_empty() {
        object.insert("properties".to_string(), Value::Object(properties));
    }
    let mut required = intersection.unwrap_or_default();
    required.extend(all_of_required);
    if !required.is_empty() {
        object.insert(
            "required".to_string(),
            Value::Array(required.into_iter().map(Value::String).collect()),
        );
    }
    if !root_has_additional && let Some(additional) = additional_properties {
        object.insert("additionalProperties".to_string(), additional);
    }
    object.insert("type".to_string(), Value::String("object".to_string()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flattens_root_unions() {
        let schema = json!({
            "anyOf": [
                {"type": "object", "properties": {"a": {"type": "string"}}, "required": ["a"]},
                {"type": "object", "properties": {"b": {"type": "integer"}}, "required": ["b"]}
            ],
            "properties": {"root": {"type": "boolean"}},
            "required": ["root"]
        });
        let normalized = normalize_object_schema(Some(&schema));
        assert_eq!(normalized["type"], "object");
        assert!(normalized["properties"].get("a").is_some());
        assert!(normalized["properties"].get("b").is_some());
        assert!(normalized["properties"].get("root").is_some());
        // anyOf required is the intersection (empty), root remains required.
        assert_eq!(normalized["required"], json!(["root"]));
    }

    #[test]
    fn resolves_local_ref_and_preserves_additional_properties() {
        let schema = json!({
            "$defs": {
                "payload": {
                    "type": "object",
                    "properties": {"value": {"type": "string"}},
                    "additionalProperties": false
                }
            },
            "allOf": [{"$ref": "#/$defs/payload"}],
            "required": ["value"]
        });
        let normalized = normalize_object_schema(Some(&schema));
        assert_eq!(normalized["type"], "object");
        assert!(normalized["properties"].get("value").is_some());
        assert_eq!(normalized["additionalProperties"], false);
    }

    #[test]
    fn resolves_nested_refs_and_stops_cycles() {
        let schema = json!({
            "$defs": {
                "base": {"$ref": "#/$defs/middle"},
                "middle": {"type": "object", "properties": {"x": {"$ref": "#/$defs/leaf"}}},
                "leaf": {"type": "string"},
                "cycle": {"$ref": "#/$defs/cycle"}
            },
            "allOf": [{"$ref": "#/$defs/base"}],
            "properties": {"loop": {"$ref": "#/$defs/cycle"}}
        });
        let normalized = normalize_object_schema(Some(&schema));
        assert_eq!(normalized["type"], "object");
        assert_eq!(normalized["properties"]["x"]["type"], "string");
        assert_eq!(normalized["properties"]["loop"], json!({}));
    }

    #[test]
    fn any_of_additional_properties_conflict_is_dropped() {
        let schema = json!({
            "anyOf": [
                {"type": "object", "additionalProperties": false},
                {"type": "object", "additionalProperties": true}
            ]
        });
        let normalized = normalize_object_schema(Some(&schema));
        assert!(normalized.get("additionalProperties").is_none());
    }

    #[test]
    fn keeps_nested_unions() {
        let schema = json!({
            "type": "object",
            "properties": {
                "nested": {"anyOf": [{"type": "string"}, {"type": "integer"}]}
            }
        });
        let normalized = normalize_object_schema(Some(&schema));
        assert!(normalized["properties"]["nested"].get("anyOf").is_some());
    }

    #[test]
    fn flattens_namespace_names_over_64_chars() {
        let name = flatten_namespace_name(
            "namespace_with_a_very_long_name_that_keeps_going",
            "tool_with_a_very_long_name",
        );
        assert!(name.chars().count() <= 64);
        assert!(name.starts_with("namespace_with_a_very_long_name"));
    }
}
