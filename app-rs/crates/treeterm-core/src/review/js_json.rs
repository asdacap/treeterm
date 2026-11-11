//! `JSON.stringify` output for values read with `JSON.parse`, so a rewrite of shared metadata
//! is byte-identical to what Electron writes for the same value.
//!
//! serde_json (with `preserve_order`) already matches it for strings, escapes and key order,
//! with two exceptions handled here: JavaScript objects list array-index keys (`"0"`, `"12"`)
//! first, in numeric order, and a number parsed from `3.0` prints as `3`.

use serde_json::{Map, Number, Value};

pub(super) fn stringify(value: &Value) -> String {
    serde_json::to_string(&normalized(value)).expect("a JSON value always serializes")
}

fn normalized(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(js_ordered(map)),
        Value::Array(items) => Value::Array(items.iter().map(normalized).collect()),
        Value::Number(number) => Value::Number(js_number(number)),
        other => other.clone(),
    }
}

fn js_ordered(map: &Map<String, Value>) -> Map<String, Value> {
    let mut indices: Vec<(u32, &String, &Value)> = Vec::new();
    let mut named: Vec<(&String, &Value)> = Vec::new();
    for (key, value) in map {
        match array_index(key) {
            Some(index) => indices.push((index, key, value)),
            None => named.push((key, value)),
        }
    }
    indices.sort_by_key(|(index, _, _)| *index);
    indices
        .into_iter()
        .map(|(_, key, value)| (key, value))
        .chain(named)
        .map(|(key, value)| (key.clone(), normalized(value)))
        .collect()
}

/// A key JavaScript treats as an array index: canonical decimal below 2^32 - 1.
fn array_index(key: &str) -> Option<u32> {
    let canonical = key == "0" || (!key.starts_with('0') && !key.is_empty());
    if !canonical || !key.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    key.parse::<u32>().ok().filter(|index| *index != u32::MAX)
}

fn js_number(number: &Number) -> Number {
    match number.as_f64() {
        Some(float)
            if number.is_f64() && float.fract() == 0.0 && float.abs() < 9_007_199_254_740_992.0 =>
        {
            Number::from(float as i64)
        }
        _ => number.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(json: &str) -> String {
        stringify(&serde_json::from_str(json).unwrap())
    }

    #[test]
    fn keeps_insertion_order_for_named_keys() {
        assert_eq!(
            roundtrip(r#"{"b":1,"a":{"d":2,"c":3}}"#),
            r#"{"b":1,"a":{"d":2,"c":3}}"#
        );
    }

    #[test]
    fn lists_array_index_keys_first_in_numeric_order() {
        assert_eq!(
            roundtrip(r#"{"b.ts":1,"10":2,"2":3,"01":4,"0":5}"#),
            r#"{"0":5,"2":3,"10":2,"b.ts":1,"01":4}"#
        );
        assert_eq!(
            roundtrip(r#"{"x":1,"4294967295":2}"#),
            r#"{"x":1,"4294967295":2}"#
        );
    }

    #[test]
    fn prints_whole_numbers_without_a_fraction() {
        assert_eq!(roundtrip(r#"[3.0,1.5,-2,1e3]"#), "[3,1.5,-2,1000]");
    }

    #[test]
    fn escapes_as_json_stringify_does() {
        assert_eq!(
            roundtrip(r#"["a\"b\\c\n\u0001 é"]"#),
            "[\"a\\\"b\\\\c\\n\\u0001\u{2028}é\"]"
        );
    }
}
