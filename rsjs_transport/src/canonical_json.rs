// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Feature-independent canonical JSON serialization for transport hashes.

use serde_json::Value;

/// Serialize a JSON value with recursively sorted object keys.
///
/// `serde_json::Map` changes its iteration order when the `preserve_order`
/// feature is enabled. Proc-macro crates and the standalone linker can acquire
/// that feature through different dependency graphs, so semantic identities
/// must never hash `Value` with `serde_json::to_vec` directly. Arrays retain
/// source order; scalar encoding delegates to `serde_json` so number and
/// string escaping remain identical to ordinary JSON serialization.
pub fn canonical_json_bytes(value: &Value) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = Vec::new();
    write_canonical_json(&mut bytes, value)?;
    Ok(bytes)
}

fn write_canonical_json(bytes: &mut Vec<u8>, value: &Value) -> Result<(), serde_json::Error> {
    match value {
        Value::Array(values) => {
            bytes.push(b'[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    bytes.push(b',');
                }
                write_canonical_json(bytes, value)?;
            }
            bytes.push(b']');
        }
        Value::Object(object) => {
            bytes.push(b'{');
            let mut entries = object.iter().collect::<Vec<_>>();
            entries.sort_unstable_by_key(|(key, _)| *key);
            for (index, (key, value)) in entries.into_iter().enumerate() {
                if index != 0 {
                    bytes.push(b',');
                }
                serde_json::to_writer(&mut *bytes, key)?;
                bytes.push(b':');
                write_canonical_json(bytes, value)?;
            }
            bytes.push(b'}');
        }
        scalar => serde_json::to_writer(bytes, scalar)?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Map, Value};

    use super::canonical_json_bytes;

    #[test]
    fn nested_object_insertion_order_does_not_change_canonical_bytes() {
        let mut forward_nested = Map::new();
        forward_nested.insert("alpha".into(), json!(1));
        forward_nested.insert("omega".into(), json!(2));
        let mut forward = Map::new();
        forward.insert("array".into(), json!([{"z": 3, "a": 4}, 5]));
        forward.insert("nested".into(), Value::Object(forward_nested));

        let mut reverse_nested = Map::new();
        reverse_nested.insert("omega".into(), json!(2));
        reverse_nested.insert("alpha".into(), json!(1));
        let mut reverse_array_object = Map::new();
        reverse_array_object.insert("a".into(), json!(4));
        reverse_array_object.insert("z".into(), json!(3));
        let mut reverse = Map::new();
        reverse.insert("nested".into(), Value::Object(reverse_nested));
        reverse.insert(
            "array".into(),
            Value::Array(vec![Value::Object(reverse_array_object), json!(5)]),
        );

        let forward = Value::Object(forward);
        let reverse = Value::Object(reverse);
        assert_ne!(
            serde_json::to_vec(&forward).unwrap(),
            serde_json::to_vec(&reverse).unwrap(),
            "the preserve_order test graph must expose the feature-dependent bytes"
        );

        let forward_bytes = canonical_json_bytes(&forward).unwrap();
        let reverse_bytes = canonical_json_bytes(&reverse).unwrap();
        assert_eq!(forward_bytes, reverse_bytes);

        let reparsed: Value = serde_json::from_slice(&forward_bytes).unwrap();
        assert_eq!(
            canonical_json_bytes(&reparsed).unwrap(),
            forward_bytes,
            "canonical encoding must survive a producer/linker JSON round trip"
        );
    }

    #[test]
    fn arrays_keep_their_source_order() {
        assert_ne!(
            canonical_json_bytes(&json!([1, 2])).unwrap(),
            canonical_json_bytes(&json!([2, 1])).unwrap()
        );
    }
}
