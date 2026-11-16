use runner_predicates::evaluate_predicate;
use serde_json::Value;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MatchKind {
    Expected,
    ExpectedAndPredicate,
}

pub fn match_output(
    actual: &Value,
    expected: Option<&Value>,
    predicate: Option<&str>,
) -> Result<MatchKind, String> {
    let expected = expected.ok_or_else(|| "expected output is required".to_string())?;
    if !matches_expected_subset(actual, expected) {
        return Err("output did not match expected subset".into());
    }
    if predicate.is_some_and(|expression| !evaluate_predicate(actual, expression)) {
        return Err("output did not satisfy predicate".into());
    }
    match predicate.is_some() {
        true => Ok(MatchKind::ExpectedAndPredicate),
        false => Ok(MatchKind::Expected),
    }
}

/// Match expected objects as recursive subsets. Arrays and scalar values keep
/// exact JSON semantics; objects may omit fields that are present in actual.
fn matches_expected_subset(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Object(actual), Value::Object(expected)) => expected.iter().all(|(key, value)| {
            actual
                .get(key)
                .is_some_and(|actual_value| matches_expected_subset(actual_value, value))
        }),
        (Value::Array(actual), Value::Array(expected)) => {
            actual.len() == expected.len()
                && actual
                    .iter()
                    .zip(expected)
                    .all(|(actual_value, expected_value)| {
                        matches_expected_subset(actual_value, expected_value)
                    })
        }
        _ => actual == expected,
    }
}

#[cfg(test)]
mod tests {
    use super::{match_output, MatchKind};
    use serde_json::json;

    #[test]
    fn expected_and_predicate_checks_must_both_pass_when_both_are_given() {
        assert_eq!(
            match_output(
                &json!({"value": 4, "extra": true}),
                Some(&json!({"value": 4})),
                None
            )
            .unwrap(),
            MatchKind::Expected
        );
        assert_eq!(
            match_output(
                &json!({"value": 4, "extra": true}),
                Some(&json!({"value": 4})),
                Some(".value >= 4")
            )
            .unwrap(),
            MatchKind::ExpectedAndPredicate
        );
        assert!(match_output(
            &json!({"value": 3}),
            Some(&json!({"value": 3})),
            Some(".value >= 4")
        )
        .is_err());
        assert!(match_output(&json!({"value": 4}), None, Some(".value >= 4")).is_err());
    }
}
