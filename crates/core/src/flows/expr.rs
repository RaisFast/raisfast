//! Minimal value/template & safe-expression evaluation (contracts.md C3).
//!
//! Pure-Rust, language-agnostic. v1 subset:
//! - refs `{{#ns.name.child#}}` (whole-string → typed value)
//! - template interpolation in strings (inline `{{#…#}}`)
//! - expressions: numbers / strings / booleans + `+ - * / % > >= < <= == !=
//!   && || ! ( )` (no free function calls — structured conditions or a script
//!   node cover richer logic).

use std::collections::HashMap;

use serde_json::Value;

use crate::errors::app_error::{AppError, AppResult};

use super::engine::Pool;

fn resolve_ref_in_pool(pool: &Pool, sel: &[&str]) -> AppResult<Value> {
    // v2 D7: a single-segment `[ns]` resolves to the node's whole namespace
    // (object of its declared fields).
    if sel.len() == 1 {
        let ns = sel[0];
        let m = pool
            .get(ns)
            .ok_or_else(|| AppError::BadRequest(format!("ref 引用不存在: {ns}")))?;
        let map: serde_json::Map<String, Value> = m.clone().into_iter().collect();
        return Ok(Value::Object(map));
    }
    if sel.is_empty() {
        return Err(AppError::BadRequest("ref 不能为空".into()));
    }
    let ns = sel[0];
    let name = sel[1];
    let mut v = pool
        .get(ns)
        .and_then(|m| m.get(name))
        .cloned()
        .ok_or_else(|| AppError::BadRequest(format!("ref 引用不存在: {ns}.{name}")))?;
    for part in &sel[2..] {
        v = v
            .get(part)
            .cloned()
            .ok_or_else(|| AppError::BadRequest(format!("ref 子路径不存在: {part}")))?;
    }
    Ok(v)
}

/// Positions + inner selector of `{{#sel#}}` tokens in a string.
///
/// `i` always sits on a UTF-8 char boundary (tokens are ASCII, so they can
/// only start on boundaries anyway); advancing byte-by-byte would slice into
/// multi-byte characters and panic on CJK text.
fn find_tokens(text: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if text[i..].starts_with("{{#")
            && let Some(rel) = text[i + 3..].find("#}}")
        {
            let inner = &text[i + 3..i + 3 + rel];
            out.push((i, i + 3 + rel + 3, inner.to_string()));
            i += 3 + rel + 3;
            continue;
        }
        i += text[i..].chars().next().map_or(1, char::len_utf8);
    }
    out
}

/// Inner selectors (`ns.field.child`) of every `{{#…#}}` token in `text`.
/// Shared with the publish-time reference lint (design D4).
#[must_use]
pub fn selectors_in_text(text: &str) -> Vec<String> {
    find_tokens(text)
        .into_iter()
        .map(|(_, _, inner)| inner)
        .collect()
}

/// Resolve a string possibly containing `{{#sel#}}`: a single whole-string
/// token returns the typed value; otherwise tokens are interpolated as text.
pub fn resolve_text(text: &str, pool: &Pool) -> AppResult<Value> {
    let tokens = find_tokens(text);
    if tokens.is_empty() {
        return Ok(Value::String(text.to_string()));
    }
    let whole = tokens.len() == 1 && tokens[0].0 == 0 && tokens[0].1 == text.len();
    if whole {
        let sel: Vec<&str> = tokens[0].2.split('.').collect();
        return resolve_ref_in_pool(pool, &sel);
    }
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for (start, end, inner) in tokens {
        out.push_str(&text[cursor..start]);
        let sel: Vec<&str> = inner.split('.').collect();
        let v = resolve_ref_in_pool(pool, &sel)?;
        out.push_str(&scalar_text(&v));
        cursor = end;
    }
    out.push_str(&text[cursor..]);
    Ok(Value::String(out))
}

fn scalar_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// Evaluate an expression string to a boolean. `{{#…#}}` refs are resolved to
/// literals before parsing (used by branch `when`/`skip_if`/`retry_if`).
pub fn eval_bool(expr: &str, pool: &Pool) -> AppResult<bool> {
    let mut normalized = String::new();
    let mut rest = expr;
    while let Some(start) = rest.find("{{#") {
        normalized.push_str(&rest[..start]);
        let after = &rest[start + 3..];
        let Some(end_rel) = after.find("#}}") else {
            return Err(AppError::BadRequest(format!("表达式含未闭合引用: {expr}")));
        };
        let sel: Vec<&str> = after[..end_rel].split('.').collect();
        let v = resolve_ref_in_pool(pool, &sel)?;
        normalized.push_str(&literal_syntax(&v)?);
        rest = &after[end_rel + 3..];
    }
    normalized.push_str(rest);
    Parser::new(&normalized).parse()
}

fn literal_syntax(v: &Value) -> AppResult<String> {
    Ok(match v {
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::String(s) => format!("{s:?}"),
        other => {
            return Err(AppError::BadRequest(format!(
                "表达式引用不支持该类型: {other}"
            )));
        }
    })
}

struct Parser<'a> {
    s: &'a str,
    b: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            s,
            b: s.as_bytes(),
            pos: 0,
        }
    }
    fn peek(&self) -> Option<char> {
        self.b.get(self.pos).copied().map(char::from)
    }
    fn skip_ws(&mut self) {
        while self.pos < self.b.len() && (self.b[self.pos] as char).is_whitespace() {
            self.pos += 1;
        }
    }
    fn eat(&mut self, c: char) -> bool {
        self.skip_ws();
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn take_op(&mut self, op: &str) -> bool {
        self.skip_ws();
        if self.s[self.pos..].starts_with(op) {
            self.pos += op.len();
            true
        } else {
            false
        }
    }
    fn parse(&mut self) -> AppResult<bool> {
        let v = self.parse_or()?;
        self.skip_ws();
        if self.pos != self.b.len() {
            return Err(AppError::BadRequest(format!(
                "表达式尾部有多余内容: {}",
                &self.s[self.pos..]
            )));
        }
        Ok(v)
    }
    fn parse_or(&mut self) -> AppResult<bool> {
        let mut v = self.parse_and()?;
        while self.take_op("||") {
            v = v || self.parse_and()?;
        }
        Ok(v)
    }
    fn parse_and(&mut self) -> AppResult<bool> {
        let mut v = self.parse_cmp()?;
        while self.take_op("&&") {
            v = v && self.parse_cmp()?;
        }
        Ok(v)
    }
    fn parse_cmp(&mut self) -> AppResult<bool> {
        let left = self.parse_arith()?;
        self.skip_ws();
        let op = if self.take_op("==") {
            Some("==")
        } else if self.take_op("!=") {
            Some("!=")
        } else if self.take_op(">=") {
            Some(">=")
        } else if self.take_op("<=") {
            Some("<=")
        } else if self.take_op(">") {
            Some(">")
        } else if self.take_op("<") {
            Some("<")
        } else {
            None
        };
        let Some(op) = op else {
            return Ok(self.as_bool(&left));
        };
        let right = self.parse_arith()?;
        Ok(cmp(op, &left, &right))
    }
    fn parse_arith(&mut self) -> AppResult<Value> {
        let mut v = self.parse_unary()?;
        loop {
            self.skip_ws();
            let c = self.peek();
            let op = match c {
                Some('+') => Some('+'),
                Some('-') => Some('-'),
                Some('*') => Some('*'),
                Some('/') => Some('/'),
                Some('%') => Some('%'),
                _ => None,
            };
            let Some(op) = op else { return Ok(v) };
            self.pos += 1;
            let r = self.parse_unary()?;
            let (a, b) = (self.num(&v)?, self.num(&r)?);
            v = Value::from(match op {
                '+' => a + b,
                '-' => a - b,
                '*' => a * b,
                '/' => a / b,
                '%' => a % b,
                _ => unreachable!(),
            });
        }
    }
    fn parse_unary(&mut self) -> AppResult<Value> {
        self.skip_ws();
        if self.eat('!') {
            let v = self.parse_unary()?;
            return Ok(Value::Bool(!self.as_bool(&v)));
        }
        if self.eat('(') {
            let v = self.parse_or()?;
            if !self.eat(')') {
                return Err(AppError::BadRequest("表达式缺 ')'".into()));
            }
            return Ok(Value::Bool(v));
        }
        self.parse_primary()
    }
    fn parse_primary(&mut self) -> AppResult<Value> {
        self.skip_ws();
        let start = self.pos;
        while self.pos < self.b.len() {
            let c = self.b[self.pos] as char;
            if c.is_whitespace()
                || matches!(
                    c,
                    ')' | '>' | '<' | '=' | '!' | '&' | '|' | '+' | '-' | '*' | '/' | '%' | '('
                )
            {
                break;
            }
            self.pos += 1;
        }
        let tok = &self.s[start..self.pos];
        if tok.is_empty() {
            return Err(AppError::BadRequest("表达式意外结束".into()));
        }
        if tok == "true" {
            return Ok(Value::Bool(true));
        }
        if tok == "false" {
            return Ok(Value::Bool(false));
        }
        if let Some(q) = tok.chars().next()
            && (q == '"' || q == '\'')
        {
            let inner = tok[1..].strip_suffix(q).unwrap_or(&tok[1..]);
            return Ok(Value::String(inner.replace("\\\"", "\"")));
        }
        tok.parse::<f64>()
            .map(Value::from)
            .map_err(|_| AppError::BadRequest(format!("表达式无法解析: {tok}")))
    }
    fn num(&self, v: &Value) -> AppResult<f64> {
        v.as_f64()
            .ok_or_else(|| AppError::BadRequest(format!("表达式需要数字，遇到 {v}")))
    }
    fn as_bool(&self, v: &Value) -> bool {
        v.as_bool()
            .unwrap_or_else(|| !v.is_null() && v.as_f64().unwrap_or(0.0) != 0.0)
    }
}

fn cmp(op: &str, l: &Value, r: &Value) -> bool {
    match (l.as_f64(), r.as_f64()) {
        (Some(a), Some(b)) => match op {
            ">" => a > b,
            ">=" => a >= b,
            "<" => a < b,
            "<=" => a <= b,
            "==" => a == b,
            "!=" => a != b,
            _ => false,
        },
        _ => match op {
            "==" => l == r,
            "!=" => l != r,
            _ => false,
        },
    }
}

/// Keep HashMap referenced so future token-cache work compiles cleanly.
#[allow(dead_code)]
fn _unused(_: HashMap<String, Value>) {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pool_of(ns: &str, name: &str, v: Value) -> Pool {
        let mut p = Pool::new();
        let mut m = HashMap::new();
        m.insert(name.to_string(), v);
        p.insert(ns.to_string(), m);
        p
    }

    #[test]
    fn text_whole_ref_is_typed() {
        let p = pool_of("start", "n", json!(42));
        assert_eq!(resolve_text("{{#start.n#}}", &p).unwrap(), json!(42));
    }

    #[test]
    fn text_interpolates_inline() {
        let p = pool_of("start", "name", json!("alice"));
        assert_eq!(
            resolve_text("hi {{#start.name#}}!", &p).unwrap(),
            json!("hi alice!")
        );
    }

    #[test]
    fn bool_expr_with_refs() {
        let p = pool_of("classify", "level", json!(5));
        assert!(eval_bool("{{#classify.level#}} >= 3", &p).unwrap());
        assert!(!eval_bool("{{#classify.level#}} >= 3 && false", &p).unwrap());
    }

    #[test]
    fn bool_expr_string_eq() {
        let p = pool_of("start", "s", json!("hi"));
        assert!(eval_bool("{{#start.s#}} == \"hi\"", &p).unwrap());
        assert!(eval_bool("{{#start.s#}} != \"x\"", &p).unwrap());
    }
}
