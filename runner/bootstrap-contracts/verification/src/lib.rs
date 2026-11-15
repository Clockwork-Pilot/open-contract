use runner_predicates::evaluate_predicate;
use serde_json::Value;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MatchKind {
    Exact,
    Predicate,
    ExpectedAndPredicate,
}

pub fn match_output(
    actual: &Value,
    expected: Option<&Value>,
    predicate: Option<&str>,
) -> Result<MatchKind, String> {
    if expected.is_some_and(|expected| actual != expected) {
        return Err("output did not equal expected value".into());
    }
    if predicate.is_some_and(|expression| !evaluate_predicate(actual, expression)) {
        return Err("output did not satisfy predicate".into());
    }
    match (expected.is_some(), predicate.is_some()) {
        (true, true) => Ok(MatchKind::ExpectedAndPredicate),
        (true, false) => Ok(MatchKind::Exact),
        (false, true) => Ok(MatchKind::Predicate),
        (false, false) => Err("expected or predicate is required".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::{match_output, MatchKind};
    use serde_json::json;

    #[test]
    fn expected_and_predicate_checks_must_both_pass_when_both_are_given() {
        assert_eq!(
            match_output(&json!({"value": 4}), Some(&json!({"value": 4})), None).unwrap(),
            MatchKind::Exact
        );
        assert_eq!(
            match_output(&json!({"value": 4}), None, Some(".value >= 4")).unwrap(),
            MatchKind::Predicate
        );
        assert!(match_output(&json!({"value": 3}), None, Some(".value >= 4")).is_err());
    }
}
