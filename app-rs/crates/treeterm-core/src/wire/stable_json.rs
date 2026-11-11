//! Deterministic JSON: object keys sorted at every level (`stableStringify` in
//! `createSessionStore.ts`), so equal values always serialize to equal strings and a body's
//! hash depends only on its content.
//!
//! Byte-identity with the Electron implementation is not required: each writer hashes the
//! exact bytes it writes, and other writers only ever compare against the daemon-reported
//! sha of those bytes.

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Serialize `value` with object keys sorted recursively.
pub fn to_string<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    let value = serde_json::to_value(value)?;
    Ok(sorted(value).to_string())
}

fn sorted(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = map.into_iter().collect();
            // JS sorts keys by UTF-16 code units; for the keys we write (ASCII identifiers,
            // tab and workspace ids) that is the same as byte order.
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(entries.into_iter().map(|(k, v)| (k, sorted(v))).collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        other => other,
    }
}

/// Lowercase-hex SHA-256 of the UTF-8 bytes, as the daemon's `sha256_hex` computes it.
pub fn sha256_hex(text: &str) -> String {
    hex::encode(Sha256::digest(text.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sorts_keys_at_every_level_and_keeps_array_order() {
        let value = json!({"b": 1, "a": {"z": [3, {"y": 1, "x": 2}], "c": null}});
        assert_eq!(
            to_string(&value).unwrap(),
            r#"{"a":{"c":null,"z":[3,{"x":2,"y":1}]},"b":1}"#
        );
    }

    #[test]
    fn hashes_like_the_daemon() {
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
