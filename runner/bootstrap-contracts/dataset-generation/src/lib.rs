use serde_json::{Map, Value};
use std::collections::BTreeSet;

const MAX_DATASET_CASES: usize = 512;
const MAX_GENERATED_STRING_LENGTH: usize = 4096;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum SchemaType {
    Null,
    Boolean,
    Object,
    Array,
    Number,
    Integer,
    String,
}

/// Generate deterministic representative inputs from the supported JSON Schema subset.
/// The output has dataset cases with `id` and `input`; it deliberately has no expectations.
pub fn generate_dataset(schema: &Value) -> Result<Value, String> {
    let inputs = values_for(schema, "$")?;
    let cases = inputs
        .into_iter()
        .enumerate()
        .map(|(index, input)| {
            serde_json::json!({"id": format!("generated-{:04}", index + 1), "input": input})
        })
        .collect::<Vec<_>>();
    Ok(serde_json::json!({"version": 1, "cases": cases}))
}

fn values_for(schema: &Value, path: &str) -> Result<Vec<Value>, String> {
    let object = schema
        .as_object()
        .ok_or_else(|| format!("{path} schema must be an object"))?;
    for keyword in object.keys() {
        if !matches!(
            keyword.as_str(),
            "$schema"
                | "$id"
                | "title"
                | "description"
                | "type"
                | "const"
                | "enum"
                | "examples"
                | "properties"
                | "required"
                | "items"
                | "prefixItems"
                | "minimum"
                | "maximum"
                | "exclusiveMinimum"
                | "exclusiveMaximum"
                | "minLength"
                | "maxLength"
        ) {
            return Err(format!("{path}.{keyword} is not supported"));
        }
    }
    if let Some(dialect) = schema.get("$schema") {
        if dialect.as_str() != Some("https://json-schema.org/draft/2020-12/schema") {
            return Err(format!(
                "{path}.$schema must identify JSON Schema draft 2020-12"
            ));
        }
    }
    for annotation in ["$id", "title", "description"] {
        if schema
            .get(annotation)
            .is_some_and(|value| !value.is_string())
        {
            return Err(format!("{path}.{annotation} must be a string"));
        }
    }
    validate_bounds(schema, path)?;

    let mut generated = if let Some(value) = schema.get("const") {
        vec![value.clone()]
    } else if let Some(values) = schema.get("enum") {
        let values = values
            .as_array()
            .ok_or_else(|| format!("{path}.enum must be an array"))?;
        if values.is_empty() {
            return Err(format!("{path}.enum must not be empty"));
        }
        ensure_unique(values, &format!("{path}.enum"))?;
        values.clone()
    } else {
        let types = schema
            .get("type")
            .ok_or_else(|| format!("{path} requires type, enum, or const"))?;
        let types = parse_types(types, path)?;
        let mut values = Vec::new();
        for kind in types {
            values.extend(values_for_type(schema, kind, path)?);
        }
        values
    };

    if let Some(examples) = schema.get("examples") {
        let examples = examples
            .as_array()
            .ok_or_else(|| format!("{path}.examples must be an array"))?;
        for example in examples {
            if !matches_schema(schema, example) {
                return Err(format!(
                    "{path}.examples contains a value outside the schema"
                ));
            }
            generated.push(example.clone());
        }
    }

    for value in &generated {
        if !matches_schema(schema, value) {
            return Err(format!("{path} generated a value outside the schema"));
        }
    }
    deduplicate(generated)
}

fn parse_types(value: &Value, path: &str) -> Result<Vec<SchemaType>, String> {
    let types = match value {
        Value::String(kind) => vec![parse_type(kind, path)?],
        Value::Array(values) if !values.is_empty() => values
            .iter()
            .map(|item| {
                let kind = item
                    .as_str()
                    .ok_or_else(|| format!("{path}.type entries must be strings"))?;
                parse_type(kind, path)
            })
            .collect::<Result<Vec<_>, _>>()?,
        Value::Array(_) => return Err(format!("{path}.type array must not be empty")),
        _ => {
            return Err(format!(
                "{path}.type must be a string or nonempty array of strings"
            ))
        }
    };
    let unique_types = types.iter().collect::<BTreeSet<_>>();
    if unique_types.len() != types.len() {
        return Err(format!("{path}.type entries must be unique"));
    }
    Ok(types)
}

fn parse_type(kind: &str, path: &str) -> Result<SchemaType, String> {
    match kind {
        "null" => Ok(SchemaType::Null),
        "boolean" => Ok(SchemaType::Boolean),
        "object" => Ok(SchemaType::Object),
        "array" => Ok(SchemaType::Array),
        "number" => Ok(SchemaType::Number),
        "integer" => Ok(SchemaType::Integer),
        "string" => Ok(SchemaType::String),
        _ => Err(format!("{path}.type {kind:?} is not supported")),
    }
}

fn ensure_unique(values: &[Value], path: &str) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for value in values {
        let encoded = serde_json::to_string(value)
            .map_err(|error| format!("cannot encode {path}: {error}"))?;
        if !seen.insert(encoded) {
            return Err(format!("{path} entries must be unique"));
        }
    }
    Ok(())
}

fn numeric_keyword(schema: &Value, key: &str, path: &str) -> Result<Option<f64>, String> {
    schema
        .get(key)
        .map(|value| {
            value
                .as_f64()
                .filter(|number| number.is_finite())
                .ok_or_else(|| format!("{path}.{key} must be a finite number"))
        })
        .transpose()
}

fn length_keyword(schema: &Value, key: &str, path: &str) -> Result<Option<usize>, String> {
    schema
        .get(key)
        .map(|value| {
            let length = value
                .as_u64()
                .ok_or_else(|| format!("{path}.{key} must be a nonnegative integer"))?;
            usize::try_from(length).map_err(|_| format!("{path}.{key} is too large to generate"))
        })
        .transpose()
}

fn validate_bounds(schema: &Value, path: &str) -> Result<(), String> {
    let minimum = numeric_keyword(schema, "minimum", path)?;
    let maximum = numeric_keyword(schema, "maximum", path)?;
    let exclusive_minimum = numeric_keyword(schema, "exclusiveMinimum", path)?;
    let exclusive_maximum = numeric_keyword(schema, "exclusiveMaximum", path)?;
    let lower = match (minimum, exclusive_minimum) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (left, right) => left.or(right),
    };
    let upper = match (maximum, exclusive_maximum) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (left, right) => left.or(right),
    };
    let lower_exclusive = exclusive_minimum.is_some_and(|value| lower == Some(value));
    let upper_exclusive = exclusive_maximum.is_some_and(|value| upper == Some(value));
    if let (Some(lower), Some(upper)) = (lower, upper) {
        if lower > upper || (lower == upper && (lower_exclusive || upper_exclusive)) {
            return Err(format!("{path} has an empty numeric range"));
        }
    }

    let min_length = length_keyword(schema, "minLength", path)?;
    let max_length = length_keyword(schema, "maxLength", path)?;
    if matches!((min_length, max_length), (Some(min), Some(max)) if min > max) {
        return Err(format!("{path}.minLength must not exceed maxLength"));
    }
    Ok(())
}

fn string_values(schema: &Value, path: &str) -> Result<Vec<Value>, String> {
    let minimum = length_keyword(schema, "minLength", path)?.unwrap_or(0);
    let maximum = length_keyword(schema, "maxLength", path)?;
    if minimum > MAX_GENERATED_STRING_LENGTH
        || maximum.is_some_and(|length| length > MAX_GENERATED_STRING_LENGTH)
    {
        return Err(format!(
            "{path} string bounds exceed the {MAX_GENERATED_STRING_LENGTH}-character generation limit"
        ));
    }
    if minimum == 0 && maximum.is_none() {
        return Ok(vec![
            Value::String(String::new()),
            Value::String("sample".into()),
        ]);
    }
    let mut lengths = vec![minimum];
    if minimum < MAX_GENERATED_STRING_LENGTH {
        lengths.push(minimum + 1);
    }
    if let Some(maximum) = maximum {
        lengths.push(maximum);
        if maximum > minimum + 1 {
            lengths.push(minimum + (maximum - minimum) / 2);
        }
    }
    if minimum == 0 {
        lengths.extend([0, "sample".chars().count()]);
    }
    lengths.sort_unstable();
    lengths.dedup();
    let values = lengths
        .into_iter()
        .filter(|length| *length >= minimum && maximum.is_none_or(|max| *length <= max))
        .map(|length| Value::String("a".repeat(length)))
        .collect();
    Ok(values)
}

fn numeric_values(schema: &Value, path: &str, integer: bool) -> Result<Vec<Value>, String> {
    let minimum = numeric_keyword(schema, "minimum", path)?;
    let maximum = numeric_keyword(schema, "maximum", path)?;
    let exclusive_minimum = numeric_keyword(schema, "exclusiveMinimum", path)?;
    let exclusive_maximum = numeric_keyword(schema, "exclusiveMaximum", path)?;
    let mut numbers = Vec::new();

    if integer {
        let lower = match (minimum, exclusive_minimum) {
            (Some(left), Some(right)) if left > right => Some((left.ceil(), false)),
            (_, Some(value)) => Some((value.floor() + 1.0, true)),
            (Some(value), None) => Some((value.ceil(), false)),
            _ => None,
        };
        let upper = match (maximum, exclusive_maximum) {
            (Some(left), Some(right)) if left < right => Some((left.floor(), false)),
            (_, Some(value)) => Some((value.ceil() - 1.0, true)),
            (Some(value), None) => Some((value.floor(), false)),
            _ => None,
        };
        if let (Some((lower, _)), Some((upper, _))) = (lower, upper) {
            if lower > upper {
                return Err(format!("{path} has no integer values in its range"));
            }
            numbers.extend([lower, ((lower + upper) / 2.0).floor(), upper]);
        } else if let Some((lower, _)) = lower {
            numbers.extend([lower, lower + 1.0]);
        } else if let Some((upper, _)) = upper {
            numbers.extend([upper - 1.0, upper]);
        } else {
            numbers.extend([0.0, 1.0]);
            numbers.push(-1.0);
        }
    } else {
        let lower = match (minimum, exclusive_minimum) {
            (Some(left), Some(right)) if left > right => Some((left, false)),
            (_, Some(value)) => Some((value, true)),
            (Some(value), None) => Some((value, false)),
            _ => None,
        };
        let upper = match (maximum, exclusive_maximum) {
            (Some(left), Some(right)) if left < right => Some((left, false)),
            (_, Some(value)) => Some((value, true)),
            (Some(value), None) => Some((value, false)),
            _ => None,
        };
        match (lower, upper) {
            (Some((low, low_exclusive)), Some((high, high_exclusive))) => {
                if !low_exclusive {
                    numbers.push(low);
                }
                let middle = low + (high - low) / 2.0;
                if middle > low && middle < high {
                    numbers.push(middle);
                }
                if !high_exclusive {
                    numbers.push(high);
                }
            }
            (Some((low, exclusive)), None) => {
                numbers.extend([low + if exclusive { 1.0 } else { 0.0 }, low + 1.0]);
            }
            (None, Some((high, exclusive))) => {
                numbers.extend([high - 1.0, high - if exclusive { 1.0 } else { 0.0 }]);
            }
            (None, None) => numbers.extend([0.0, 1.0, -1.0]),
        }
    }

    let mut values = Vec::new();
    for number in numbers {
        let number = if integer { number.round() } else { number };
        let json_number = if integer {
            let integer = number as i64;
            serde_json::Number::from(integer)
        } else {
            serde_json::Number::from_f64(number)
                .ok_or_else(|| format!("{path} produced a non-finite number"))?
        };
        let value = Value::Number(json_number);
        if matches_schema(schema, &value) && !values.contains(&value) {
            values.push(value);
        }
    }
    if values.is_empty() {
        return Err(format!("{path} has no generated values within its range"));
    }
    Ok(values)
}

fn values_for_type(schema: &Value, kind: SchemaType, path: &str) -> Result<Vec<Value>, String> {
    match kind {
        SchemaType::Null => Ok(vec![Value::Null]),
        SchemaType::Boolean => Ok(vec![Value::Bool(false), Value::Bool(true)]),
        SchemaType::String => string_values(schema, path),
        SchemaType::Integer => numeric_values(schema, path, true),
        SchemaType::Number => numeric_values(schema, path, false),
        SchemaType::Array => array_values(schema, path),
        SchemaType::Object => object_values(schema, path),
    }
}

fn array_values(schema: &Value, path: &str) -> Result<Vec<Value>, String> {
    let prefix = match schema.get("prefixItems") {
        None => Vec::new(),
        Some(value) => value
            .as_array()
            .ok_or_else(|| format!("{path}.prefixItems must be an array"))?
            .iter()
            .enumerate()
            .map(|(index, item)| values_for(item, &format!("{path}.prefixItems[{index}]")))
            .collect::<Result<Vec<_>, _>>()?,
    };
    let item_values = schema
        .get("items")
        .map(|item| values_for(item, &format!("{path}.items")))
        .transpose()?;

    let mut arrays = vec![Vec::new()];
    let mut prefix_arrays = vec![Vec::new()];
    for choices in prefix {
        prefix_arrays = extend_arrays(prefix_arrays, choices)?;
        arrays.extend(prefix_arrays.iter().cloned());
    }
    if let Some(items) = item_values {
        let has_prefix = schema
            .get("prefixItems")
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty());
        let bases = if has_prefix {
            prefix_arrays
        } else {
            vec![Vec::new()]
        };
        for base in bases {
            for item in &items {
                if arrays.len() >= MAX_DATASET_CASES {
                    return Err(format!("schema expands beyond {MAX_DATASET_CASES} cases"));
                }
                let mut candidate = base.clone();
                candidate.push(item.clone());
                arrays.push(candidate);
            }
        }
    }
    if arrays.len() > MAX_DATASET_CASES {
        return Err(format!("schema expands beyond {MAX_DATASET_CASES} cases"));
    }
    Ok(arrays.into_iter().map(Value::Array).collect())
}

fn extend_arrays(current: Vec<Vec<Value>>, choices: Vec<Value>) -> Result<Vec<Vec<Value>>, String> {
    let mut next = Vec::new();
    for prefix in current {
        for choice in &choices {
            if next.len() >= MAX_DATASET_CASES {
                return Err(format!("schema expands beyond {MAX_DATASET_CASES} cases"));
            }
            let mut candidate = prefix.clone();
            candidate.push(choice.clone());
            next.push(candidate);
        }
    }
    Ok(next)
}

fn object_values(schema: &Value, path: &str) -> Result<Vec<Value>, String> {
    let properties = match schema.get("properties") {
        None => None,
        Some(properties) => Some(
            properties
                .as_object()
                .ok_or_else(|| format!("{path}.properties must be an object"))?,
        ),
    };
    let required = schema
        .get("required")
        .map(|value| {
            value
                .as_array()
                .ok_or_else(|| format!("{path}.required must be an array"))
        })
        .transpose()?
        .into_iter()
        .flatten()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{path}.required entries must be strings"))
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    if let Some(required_values) = schema.get("required").and_then(Value::as_array) {
        if required.len() != required_values.len() {
            return Err(format!("{path}.required entries must be unique"));
        }
    }
    let mut combinations = vec![Map::new()];
    if let Some(properties) = properties {
        for name in &required {
            if !properties.contains_key(name) {
                return Err(format!("{path}.required names unknown property {name:?}"));
            }
        }
        for (name, property_schema) in properties {
            let property_values =
                values_for(property_schema, &format!("{path}.properties.{name}"))?;
            combinations = bounded_cartesian(
                combinations,
                name,
                property_values,
                !required.contains(name),
            )?;
        }
    } else if !required.is_empty() {
        return Err(format!("{path}.required needs properties"));
    }
    Ok(combinations.into_iter().map(Value::Object).collect())
}

fn bounded_cartesian(
    current: Vec<Map<String, Value>>,
    name: &str,
    property_values: Vec<Value>,
    optional: bool,
) -> Result<Vec<Map<String, Value>>, String> {
    let mut next = Vec::new();
    for object in current {
        if optional {
            next.push(object.clone());
        }
        for value in &property_values {
            if next.len() >= MAX_DATASET_CASES {
                return Err(format!("schema expands beyond {MAX_DATASET_CASES} cases"));
            }
            let mut variant = object.clone();
            variant.insert(name.to_owned(), value.clone());
            next.push(variant);
        }
    }
    Ok(next)
}

fn matches_schema(schema: &Value, value: &Value) -> bool {
    if let Some(constant) = schema.get("const") {
        if constant != value {
            return false;
        }
    }
    if let Some(enumeration) = schema.get("enum").and_then(Value::as_array) {
        if !enumeration.contains(value) {
            return false;
        }
    }
    if let Some(types) = schema.get("type") {
        let types = match parse_types(types, "$example") {
            Ok(types) => types,
            Err(_) => return false,
        };
        if !types.iter().any(|kind| matches_type(kind, value)) {
            return false;
        }
    }
    if let Some(number) = value.as_f64() {
        if schema
            .get("minimum")
            .and_then(Value::as_f64)
            .is_some_and(|minimum| number < minimum)
            || schema
                .get("maximum")
                .and_then(Value::as_f64)
                .is_some_and(|maximum| number > maximum)
            || schema
                .get("exclusiveMinimum")
                .and_then(Value::as_f64)
                .is_some_and(|minimum| number <= minimum)
            || schema
                .get("exclusiveMaximum")
                .and_then(Value::as_f64)
                .is_some_and(|maximum| number >= maximum)
        {
            return false;
        }
    }
    if let Some(string) = value.as_str() {
        let length = string.chars().count();
        if schema
            .get("minLength")
            .and_then(Value::as_u64)
            .is_some_and(|minimum| (length as u64) < minimum)
            || schema
                .get("maxLength")
                .and_then(Value::as_u64)
                .is_some_and(|maximum| (length as u64) > maximum)
        {
            return false;
        }
    }
    if let Some(properties) = schema.get("properties").and_then(Value::as_object) {
        if let Some(object) = value.as_object() {
            if let Some(required) = schema.get("required").and_then(Value::as_array) {
                for name in required.iter().filter_map(Value::as_str) {
                    if !object.contains_key(name) {
                        return false;
                    }
                }
            }
            for (name, property_schema) in properties {
                if let Some(property) = object.get(name) {
                    if !matches_schema(property_schema, property) {
                        return false;
                    }
                }
            }
        } else {
            return false;
        }
    }
    if let Some(items) = value.as_array() {
        if let Some(prefix) = schema.get("prefixItems").and_then(Value::as_array) {
            for (index, item) in items.iter().enumerate().take(prefix.len()) {
                if !matches_schema(&prefix[index], item) {
                    return false;
                }
            }
        }
        if let Some(item_schema) = schema.get("items") {
            let prefix_len = schema
                .get("prefixItems")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            if items
                .iter()
                .skip(prefix_len)
                .any(|item| !matches_schema(item_schema, item))
            {
                return false;
            }
        }
    }
    true
}

fn matches_type(kind: &SchemaType, value: &Value) -> bool {
    match kind {
        SchemaType::Null => value.is_null(),
        SchemaType::Boolean => value.is_boolean(),
        SchemaType::Object => value.is_object(),
        SchemaType::Array => value.is_array(),
        SchemaType::Number => value.is_number(),
        SchemaType::Integer => value.as_i64().is_some() || value.as_u64().is_some(),
        SchemaType::String => value.is_string(),
    }
}

fn deduplicate(values: Vec<Value>) -> Result<Vec<Value>, String> {
    let mut seen = BTreeSet::new();
    let mut unique = Vec::new();
    for value in values {
        let encoded = serde_json::to_string(&value)
            .map_err(|error| format!("cannot encode generated input: {error}"))?;
        if seen.insert(encoded) {
            if unique.len() == MAX_DATASET_CASES {
                return Err(format!(
                    "generated dataset exceeds {MAX_DATASET_CASES} cases"
                ));
            }
            unique.push(value);
        }
    }
    Ok(unique)
}
