//! String-based predicate evaluator.
use serde_json::{Map, Number, Value};
use std::cmp::Ordering;

fn compare_values(left: &Value, right: &Value) -> Option<Ordering> {
    if left == right {
        return Some(Ordering::Equal);
    }
    match (left, right) {
        (Value::Number(a), Value::Number(b)) => number_cmp(a, b),
        (Value::String(a), Value::String(b)) => {
            let a_digest = a.strip_prefix("sha256:").unwrap_or(a);
            let b_digest = b.strip_prefix("sha256:").unwrap_or(b);
            if a_digest.len() == 64
                && b_digest.len() == 64
                && a_digest.bytes().all(|ch| ch.is_ascii_hexdigit())
                && b_digest.bytes().all(|ch| ch.is_ascii_hexdigit())
            {
                Some(
                    a_digest
                        .to_ascii_lowercase()
                        .cmp(&b_digest.to_ascii_lowercase()),
                )
            } else {
                Some(a.cmp(b))
            }
        }
        (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(b)),
        _ => None,
    }
}

fn value_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::String(_), Value::String(_)) => {
            compare_values(left, right) == Some(Ordering::Equal)
        }
        (Value::Number(left), Value::Number(right)) => {
            number_cmp(left, right) == Some(Ordering::Equal)
        }
        _ => left == right,
    }
}

pub(crate) fn number_cmp(left: &Number, right: &Number) -> Option<Ordering> {
    match (left.as_i64(), right.as_i64(), left.as_u64(), right.as_u64()) {
        (Some(a), Some(b), _, _) => Some(a.cmp(&b)),
        (_, _, Some(a), Some(b)) => Some(a.cmp(&b)),
        _ => {
            let left = left.as_f64().map_or(f64::NAN, |value| value);
            let right = right.as_f64().map_or(f64::NAN, |value| value);
            left.partial_cmp(&right)
        }
    }
}

pub(crate) fn eval_expression(record: &Value, expression: &str) -> bool {
    let expression = strip_outer_parens(expression.trim());
    if expression == "true" {
        return true;
    }
    if expression == "false" || expression.is_empty() {
        return false;
    }
    if let Some(parts) = split_boolean(expression, " OR ") {
        return parts.into_iter().any(|part| eval_expression(record, part));
    }
    if let Some(parts) = split_boolean(expression, " AND ") {
        return parts.into_iter().all(|part| eval_expression(record, part));
    }
    if let Some(rest) = expression.strip_prefix("NOT ") {
        return !eval_expression(record, rest);
    }
    if let Some(arg) = expression
        .strip_prefix("exists(")
        .and_then(|v| v.strip_suffix(')'))
    {
        return resolve_expression_field(record, arg.trim()).is_some_and(|v| {
            !v.is_null()
                && !v.as_object().is_some_and(Map::is_empty)
                && !v.as_array().is_some_and(Vec::is_empty)
        });
    }
    for (symbol, operator) in [
        (" == ", ComparisonOperator::Equal),
        (" != ", ComparisonOperator::NotEqual),
        (" >= ", ComparisonOperator::GreaterOrEqual),
        (" <= ", ComparisonOperator::LessOrEqual),
        (" > ", ComparisonOperator::Greater),
        (" < ", ComparisonOperator::Less),
    ] {
        if let Some((left, right)) = split_comparison(expression, symbol) {
            let left = left.trim();
            let right = right.trim();
            let lhs = resolve_expression_field(record, left);
            let rhs = if right.starts_with('.') || right.starts_with('[') {
                Some(
                    resolve_expression_field(record, right)
                        .cloned()
                        .unwrap_or(Value::Null),
                )
            } else {
                parse_expression_literal(right)
            };
            let Some(rhs) = rhs else {
                return false;
            };
            return match operator {
                ComparisonOperator::Equal => value_equal(lhs.unwrap_or(&Value::Null), &rhs),
                ComparisonOperator::NotEqual => !value_equal(lhs.unwrap_or(&Value::Null), &rhs),
                ComparisonOperator::Greater => {
                    compare_optional(lhs, &rhs, |o| o == Ordering::Greater)
                }
                ComparisonOperator::GreaterOrEqual => {
                    compare_optional(lhs, &rhs, |o| o != Ordering::Less)
                }
                ComparisonOperator::Less => compare_optional(lhs, &rhs, |o| o == Ordering::Less),
                ComparisonOperator::LessOrEqual => {
                    compare_optional(lhs, &rhs, |o| o != Ordering::Greater)
                }
            };
        }
    }
    false
}

#[derive(Clone, Copy)]
enum ComparisonOperator {
    Equal,
    NotEqual,
    Greater,
    GreaterOrEqual,
    Less,
    LessOrEqual,
}

fn strip_outer_parens(mut text: &str) -> &str {
    loop {
        if !text.starts_with('(') || !text.ends_with(')') {
            return text;
        }
        let mut depth = 0_i32;
        let mut quote = false;
        let mut encloses_all = true;
        for (index, ch) in text.char_indices() {
            if ch == '\'' {
                quote = !quote;
            }
            if quote {
                continue;
            }
            if ch == '(' {
                depth += 1;
            }
            if ch == ')' {
                depth -= 1;
            }
            if depth == 0 && index + ch.len_utf8() < text.len() {
                encloses_all = false;
                break;
            }
        }
        if !encloses_all {
            return text;
        }
        text = &text[1..text.len() - 1];
    }
}

fn split_boolean<'a>(text: &'a str, separator: &str) -> Option<Vec<&'a str>> {
    let mut parts = Vec::new();
    let (mut depth, mut quoted, mut start) = (0_i32, false, 0);
    for (index, ch) in text.char_indices() {
        if ch == '\'' {
            quoted = !quoted;
        }
        if !quoted {
            if ch == '(' {
                depth += 1;
            }
            if ch == ')' {
                depth -= 1;
            }
            if depth == 0 && text[index..].starts_with(separator) {
                parts.push(text[start..index].trim());
                start = index + separator.len();
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        parts.push(text[start..].trim());
        Some(parts)
    }
}

fn split_comparison<'a>(text: &'a str, operator: &str) -> Option<(&'a str, &'a str)> {
    for (index, _ch) in text.char_indices() {
        if text[index..].starts_with(operator) {
            return Some((&text[..index], &text[index + operator.len()..]));
        }
    }
    None
}

fn resolve_expression_field<'a>(record: &'a Value, expression: &str) -> Option<&'a Value> {
    record.pointer(&parse_field_pointer(expression)?)
}

fn parse_field_pointer(expression: &str) -> Option<String> {
    let mut chars = expression.char_indices().peekable();
    let mut pointer = String::new();
    let mut found = false;
    while let Some((_, ch)) = chars.next() {
        let segment = if ch == '.' {
            let start = chars.peek()?.0;
            while chars
                .peek()
                .is_some_and(|(_, next)| *next != '.' && *next != '[')
            {
                chars.next();
            }
            let end = chars.peek().map_or(expression.len(), |(index, _)| *index);
            if end == start {
                return None;
            }
            expression[start..end].to_string()
        } else if ch == '[' {
            let start = chars.peek()?.0;
            let mut quoted = false;
            let mut escaped = false;
            let mut end = None;
            for (index, next) in chars.by_ref() {
                if quoted {
                    if escaped {
                        escaped = false;
                    } else if next == '\\' {
                        escaped = true;
                    } else if next == '"' {
                        quoted = false;
                    }
                } else if next == '"' {
                    quoted = true;
                } else if next == ']' {
                    end = Some(index);
                    break;
                }
            }
            let end = end?;
            let content = &expression[start..end];
            serde_json::from_str::<String>(content).ok()?
        } else {
            return None;
        };
        pointer.push('/');
        pointer.push_str(&escape_pointer_part(&segment));
        found = true;
    }
    found.then_some(pointer)
}

fn parse_expression_literal(text: &str) -> Option<Value> {
    if text.starts_with('\'') && text.ends_with('\'') && text.len() >= 2 {
        return Some(Value::String(
            text[1..text.len() - 1]
                .replace("\\'", "'")
                .replace("\\\\", "\\"),
        ));
    }
    serde_json::from_str(text).ok()
}

fn escape_pointer_part(part: &str) -> String {
    part.replace('~', "~0").replace('/', "~1")
}

fn compare_optional(
    value: Option<&Value>,
    operand: &Value,
    accepts: impl FnOnce(Ordering) -> bool,
) -> bool {
    let is_orderable =
        |value: &Value| matches!(value, Value::Bool(_) | Value::Number(_) | Value::String(_));
    let Some(value) = value.filter(|value| is_orderable(value)) else {
        return false;
    };
    if !is_orderable(operand) {
        return false;
    }
    compare_values(value, operand).is_some_and(accepts)
}

pub fn evaluate_predicate(record: &Value, predicate: &str) -> bool {
    eval_expression(record, predicate)
}
