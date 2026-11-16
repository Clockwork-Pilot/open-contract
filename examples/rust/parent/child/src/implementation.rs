use serde_json::{json, Value};

pub fn sum(input: &Value) -> Result<Value, String> {
    let values = input
        .get("values")
        .and_then(Value::as_array)
        .ok_or_else(|| "values must be an array".to_string())?;
    let total = values
        .iter()
        .map(|value| {
            value
                .as_i64()
                .ok_or_else(|| "values must contain integers".to_string())
        })
        .try_fold(0_i64, |sum, value| {
            value.and_then(|value| {
                sum.checked_add(value)
                    .ok_or_else(|| "sum overflow".to_string())
            })
        })?;
    Ok(json!({ "total": total }))
}
