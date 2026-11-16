use serde_json::Value;

pub fn validate_case(item: &Value) -> Result<Value, String> {
    super::implementation::sum(&item["input"])
}
