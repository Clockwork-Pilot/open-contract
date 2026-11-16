use serde_json::{json, Value};

pub fn greeting(input: &Value) -> Result<Value, String> {
    let name = input
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "name must be a string".to_string())?;
    Ok(json!({ "greeting": format!("hello, {name}") }))
}
