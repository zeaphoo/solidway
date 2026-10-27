//! 方言的字段级 patch：只做 JSON 形状操作，不做语义转换。

use serde_json::{Map, Value};

use crate::report::{Error, Report, Side};

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub(crate) enum FieldOp {
    Rename {
        from: &'static str,
        to: &'static str,
    },
    Wrap {
        path: &'static str,
        key: &'static str,
    },
    Unwrap {
        path: &'static str,
        key: &'static str,
    },
    Default {
        path: &'static str,
        value: &'static str,
    },
    Set {
        path: &'static str,
        value: &'static str,
    },
    Drop {
        path: &'static str,
        reason: &'static str,
    },
}

pub(crate) fn apply_ops(body: &[u8], ops: &[FieldOp]) -> Result<(Vec<u8>, Report), Error> {
    let mut value: Value = serde_json::from_slice(body)?;
    if !value.is_object() {
        return Err(Error::Invalid(
            "dialect patch expects a JSON object".to_string(),
        ));
    }
    let mut report = Report::default();
    for op in ops {
        apply_op(&mut value, op, &mut report)?;
    }
    Ok((serde_json::to_vec(&value)?, report))
}

fn apply_op(value: &mut Value, op: &FieldOp, report: &mut Report) -> Result<(), Error> {
    match *op {
        FieldOp::Rename { from, to } => {
            if let Some(field) = remove(value, from) {
                set(value, to, field)?;
            }
        }
        FieldOp::Wrap { path, key } => {
            if let Some(field) = remove(value, path) {
                let mut object = Map::new();
                object.insert(key.to_string(), field);
                set(value, path, Value::Object(object))?;
            }
        }
        FieldOp::Unwrap { path, key } => {
            if let Some(field) = remove(value, path) {
                match field {
                    Value::Object(mut object) => {
                        let unwrapped = object.remove(key).unwrap_or(Value::Null);
                        set(value, path, unwrapped)?;
                    }
                    other => set(value, path, other)?,
                }
            }
        }
        FieldOp::Default { path, value: raw } => {
            if get(value, path).is_none() {
                set(value, path, parse(raw)?)?;
            }
        }
        FieldOp::Set { path, value: raw } => {
            set(value, path, parse(raw)?)?;
        }
        FieldOp::Drop { path, reason } => {
            if remove(value, path).is_some() {
                report.dropped(Side::Source, path, reason);
            }
        }
    }
    Ok(())
}

fn parse(raw: &str) -> Result<Value, Error> {
    serde_json::from_str(raw)
        .map_err(|err| Error::Invalid(format!("invalid dialect patch value: {err}")))
}

fn tokens(pointer: &str) -> Vec<String> {
    if pointer.is_empty() || pointer == "/" {
        return Vec::new();
    }
    pointer
        .trim_start_matches('/')
        .split('/')
        .map(|token| token.replace("~1", "/").replace("~0", "~"))
        .collect()
}

fn get<'a>(value: &'a Value, pointer: &str) -> Option<&'a Value> {
    let mut current = value;
    for token in tokens(pointer) {
        current = match current {
            Value::Object(object) => object.get(&token)?,
            Value::Array(items) => items.get(token.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(current)
}

fn remove(value: &mut Value, pointer: &str) -> Option<Value> {
    let tokens = tokens(pointer);
    let (last, parents) = tokens.split_last()?;
    let mut current = value;
    for token in parents {
        current = match current {
            Value::Object(object) => object.get_mut(token)?,
            Value::Array(items) => items.get_mut(token.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    match current {
        Value::Object(object) => object.remove(last),
        Value::Array(items) => {
            let index = last.parse::<usize>().ok()?;
            (index < items.len()).then(|| items.remove(index))
        }
        _ => None,
    }
}

fn set(value: &mut Value, pointer: &str, new_value: Value) -> Result<(), Error> {
    let tokens = tokens(pointer);
    let (last, parents) = tokens
        .split_last()
        .ok_or_else(|| Error::Invalid("dialect patch cannot replace the root".to_string()))?;
    let mut current = value;
    for token in parents {
        if !current.is_object() && !current.is_array() {
            *current = Value::Object(Map::new());
        }
        current = match current {
            Value::Object(object) => object
                .entry(token.clone())
                .or_insert_with(|| Value::Object(Map::new())),
            Value::Array(items) => {
                let index = token
                    .parse::<usize>()
                    .map_err(|_| Error::Invalid(format!("invalid array index {token}")))?;
                if index >= items.len() {
                    items.resize(index + 1, Value::Null);
                }
                &mut items[index]
            }
            _ => {
                return Err(Error::Invalid(format!(
                    "cannot traverse {token} in JSON patch"
                )));
            }
        };
    }
    match current {
        Value::Object(object) => {
            object.insert(last.clone(), new_value);
        }
        Value::Array(items) => {
            let index = last
                .parse::<usize>()
                .map_err(|_| Error::Invalid(format!("invalid array index {last}")))?;
            if index >= items.len() {
                items.resize(index + 1, Value::Null);
            }
            items[index] = new_value;
        }
        _ => {
            return Err(Error::Invalid(format!(
                "cannot set {pointer} on a non-container"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_wrap_unwrap_default_set_and_drop() {
        let ops = [
            FieldOp::Rename {
                from: "/old",
                to: "/new",
            },
            FieldOp::Wrap {
                path: "/new",
                key: "inner",
            },
            FieldOp::Unwrap {
                path: "/new",
                key: "inner",
            },
            FieldOp::Default {
                path: "/missing",
                value: "true",
            },
            FieldOp::Set {
                path: "/forced",
                value: "\"value\"",
            },
            FieldOp::Drop {
                path: "/drop_me",
                reason: "test",
            },
        ];
        let (body, report) = apply_ops(br#"{"old":1,"drop_me":2}"#, &ops).unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["new"], 1);
        assert_eq!(value["missing"], true);
        assert_eq!(value["forced"], "value");
        assert!(value.get("drop_me").is_none());
        assert_eq!(report.warnings.len(), 1);
    }
}
