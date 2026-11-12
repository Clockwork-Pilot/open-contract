use open_contracts_predicates::evaluate_predicate;
use serde_json::Value;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MatchKind {
    Exact,
    Predicate,
}

pub fn match_output(
    actual: &Value,
    expected: Option<&Value>,
    predicate: Option<&str>,
) -> Result<MatchKind, String> {
    match (expected, predicate) {
        (Some(_), Some(_)) | (None, None) => {
            Err("exactly one of expected or predicate is required".into())
        }
        (Some(expected), None) if actual == expected => Ok(MatchKind::Exact),
        (Some(_), None) => Err("output did not equal expected value".into()),
        (None, Some(expression)) if evaluate_predicate(actual, expression) => {
            Ok(MatchKind::Predicate)
        }
        (None, Some(_)) => Err("output did not satisfy predicate".into()),
    }
}
