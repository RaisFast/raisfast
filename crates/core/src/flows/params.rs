//! Start-parameter validation & defaulting (contracts C1.4b, Dify-inspired).
//!
//! Applied at run entry before seeding the pool: `required` missing → error,
//! missing optional → default, then type checks (text/paragraph/select/number/
//! boolean/…). Callers mutate the start-namespace map in place.

use std::collections::HashMap;

use serde_json::Value;

use crate::errors::app_error::{AppError, AppResult};

use super::nodes::StartParam;

/// Validate + default `inputs` against the declared start params.
pub fn apply(params: &[StartParam], inputs: &mut HashMap<String, Value>) -> AppResult<()> {
    for p in params {
        let present = inputs.contains_key(&p.variable);
        if !present {
            match (&p.default, p.required) {
                (Some(d), _) => {
                    inputs.insert(p.variable.clone(), d.clone());
                }
                (None, true) => {
                    return Err(AppError::BadRequest(format!(
                        "start 入参 '{}' 必填",
                        p.variable
                    )));
                }
                (None, false) => continue,
            }
        }
        let v = inputs.get_mut(&p.variable).unwrap();
        validate_one(p, v)?;
    }
    Ok(())
}

fn validate_one(p: &StartParam, v: &mut Value) -> AppResult<()> {
    match p.kind.as_str() {
        "text" | "paragraph" | "select" => {
            let s = v
                .as_str()
                .ok_or_else(|| AppError::BadRequest(format!("入参 '{}' 需为字符串", p.variable)))?;
            if let Some(max) = p.max_length
                && (s.len() as i64) > max
            {
                return Err(AppError::BadRequest(format!(
                    "入参 '{}' 超过 max_length={max}",
                    p.variable
                )));
            }
            if p.kind == "select"
                && let Some(opts) = &p.options
                && !opts.iter().any(|o| o.as_str() == Some(s))
            {
                return Err(AppError::BadRequest(format!(
                    "入参 '{}' 不在 options 内",
                    p.variable
                )));
            }
        }
        "number" => {
            if let Some(s) = v.as_str() {
                let n = s
                    .parse::<f64>()
                    .map_err(|_| AppError::BadRequest(format!("入参 '{}' 不是数字", p.variable)))?;
                *v = serde_json::json!(n);
            } else if !v.is_number() {
                return Err(AppError::BadRequest(format!(
                    "入参 '{}' 需为数字",
                    p.variable
                )));
            }
        }
        "boolean" => {
            if !v.is_boolean() {
                let b = match v.as_str() {
                    Some(s) => matches!(s, "true" | "1" | "on"),
                    None => {
                        return Err(AppError::BadRequest(format!(
                            "入参 '{}' 需为布尔",
                            p.variable
                        )));
                    }
                };
                *v = Value::Bool(b);
            }
        }
        "json" if !v.is_object() => {
            return Err(AppError::BadRequest(format!(
                "入参 '{}' 需为对象",
                p.variable
            )));
        }
        "json" => {}
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sp(name: &str, kind: &str, required: bool, default: Option<Value>) -> StartParam {
        StartParam {
            variable: name.into(),
            label: String::new(),
            kind: kind.into(),
            required,
            default,
            max_length: None,
            options: None,
            accept: None,
            max_count: None,
        }
    }

    #[test]
    fn required_missing_errors() {
        let p = [sp("msg", "paragraph", true, None)];
        let mut m = HashMap::new();
        assert!(apply(&p, &mut m).is_err());
    }

    #[test]
    fn default_filled_and_number_coerced() {
        let p = [
            sp("level", "number", false, Some(json!(2))),
            sp("msg", "text", true, None),
        ];
        let mut m = HashMap::new();
        m.insert("msg".into(), json!("hi"));
        apply(&p, &mut m).unwrap();
        assert_eq!(m["level"], 2);
        // numeric string coerced
        m.insert("level".into(), json!("5"));
        apply(&p, &mut m).unwrap();
        assert_eq!(m["level"], 5.0);
    }
}
