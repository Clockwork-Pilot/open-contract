use serde_json::Value;

pub fn validate_case(input: &Value, _context: &Value) -> Result<Value, String> {
    super::implementation::sum(input)
}
