// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use rsjs::{ClientDecode, ClientEncode};
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(rsjs::ClientProps)]
struct CorpusTag {
    label: String,
}

#[derive(rsjs::ClientProps)]
struct CorpusAccount {
    balance_cents: u64,
    bonus: Option<i64>,
    tags: Vec<CorpusTag>,
    primary_tag: Option<CorpusTag>,
}

#[derive(rsjs::ClientProps)]
enum CorpusStatus {
    Idle,
    Ready,
}

pub fn parse_and_validate(source: &str) -> Result<Value, String> {
    let corpus: Value = serde_json::from_str(source)
        .map_err(|err| format!("invalid semantic corpus JSON: {err}"))?;
    if corpus.get("schema_version").and_then(Value::as_u64) != Some(1) {
        return Err("semantic corpus must declare schema_version = 1".to_string());
    }

    let cases = corpus
        .get("cases")
        .and_then(Value::as_array)
        .ok_or_else(|| "semantic corpus needs a cases array".to_string())?;
    let exclusions = corpus
        .get("exclusions")
        .and_then(Value::as_array)
        .ok_or_else(|| "semantic corpus needs an exclusions array".to_string())?;
    let mut ids = BTreeSet::new();

    for case in cases {
        let id = required_str(case, "id")?;
        if !ids.insert(id.to_string()) {
            return Err(format!("duplicate semantic corpus id `{id}`"));
        }
        match required_str(case, "expectation")? {
            "parity" => {}
            "known_divergence" => {
                required_str(case, "reason")?;
                if case.get("browser_characterization").is_none() {
                    return Err(format!(
                        "known-divergence case `{id}` needs browser_characterization"
                    ));
                }
            }
            other => {
                return Err(format!(
                    "semantic corpus case `{id}` has unknown expectation `{other}`"
                ));
            }
        }
        required_str(case, "operation")?;
        if !case.get("input").is_some_and(Value::is_object) {
            return Err(format!("semantic corpus case `{id}` needs an object input"));
        }
    }

    for exclusion in exclusions {
        let id = required_str(exclusion, "id")?;
        if !ids.insert(id.to_string()) {
            return Err(format!("duplicate semantic corpus id `{id}`"));
        }
        match required_str(exclusion, "kind")? {
            "characterization" | "known_gap" | "compile_fail" => {}
            other => {
                return Err(format!(
                    "semantic corpus exclusion `{id}` has unknown kind `{other}`"
                ));
            }
        }
        required_str(exclusion, "reason")?;
    }

    Ok(corpus)
}

pub fn evaluate(corpus: &Value) -> Result<Value, String> {
    let cases = corpus
        .get("cases")
        .and_then(Value::as_array)
        .ok_or_else(|| "semantic corpus needs a cases array".to_string())?;
    let mut results = Vec::with_capacity(cases.len());

    for case in cases {
        let id = required_str(case, "id")?;
        let operation = required_str(case, "operation")?;
        let input = case
            .get("input")
            .ok_or_else(|| format!("semantic corpus case `{id}` needs input"))?;
        let value = evaluate_case(operation, input)
            .map_err(|err| format!("semantic corpus case `{id}` failed: {err}"))?;
        results.push(json!({
            "id": id,
            "value": value,
        }));
    }

    Ok(json!({
        "schema_version": 1,
        "results": results,
    }))
}

fn evaluate_case(operation: &str, input: &Value) -> Result<Value, String> {
    match operation {
        "string_utf8_len" => {
            let value = input_string(input, "value")?;
            Ok(json!(value.len()))
        }
        "string_trim" => {
            let value = input_string(input, "value")?;
            Ok(json!(value.trim()))
        }
        "i64_increment" => {
            let value = parse_i64(input, "value")?;
            let delta = parse_i64(input, "delta")?;
            let result = value
                .checked_add(delta)
                .ok_or_else(|| "i64 increment overflowed the Rust domain".to_string())?;
            Ok(json!(result.to_string()))
        }
        "i64_div_rem" => {
            let lhs = parse_i64(input, "lhs")?;
            let rhs = parse_i64(input, "rhs")?;
            if rhs == 0 || (lhs == i64::MIN && rhs == -1) {
                return Err("division case is outside the non-panicking Rust domain".to_string());
            }
            Ok(json!({
                // Keep the differential transport exact past JavaScript's
                // safe-integer boundary. The client side compares the
                // checked BigInt results through the same decimal wire form.
                "quotient": (lhs / rhs).to_string(),
                "remainder": (lhs % rhs).to_string(),
            }))
        }
        "f64_display" => {
            let value = match input_string(input, "value")? {
                "sum_0_1_0_2" => 0.1_f64 + 0.2_f64,
                "nan" => f64::NAN,
                "positive_infinity" => f64::INFINITY,
                "negative_infinity" => f64::NEG_INFINITY,
                "negative_zero" => -0.0_f64,
                "small_exponent" => 1.0e-7_f64,
                token => return Err(format!("unknown f64 display token `{token}`")),
            };
            Ok(json!(format!("{value}")))
        }
        "option_i32_wire" => {
            let value = match input.get("value") {
                Some(Value::Null) => None,
                Some(value) => Some(
                    value
                        .as_i64()
                        .and_then(|value| i32::try_from(value).ok())
                        .ok_or_else(|| "option_i32_wire value must fit i32".to_string())?,
                ),
                None => return Err("option_i32_wire input needs value".to_string()),
            };
            wire_roundtrip(value)
        }
        "option_string_wire" => {
            let value = match input.get("value") {
                Some(Value::Null) => None,
                Some(Value::String(value)) => Some(value.clone()),
                _ => return Err("option_string_wire value must be a string or null".to_string()),
            };
            wire_roundtrip(value)
        }
        "fieldless_enum_wire" => {
            let value = match input_string(input, "value")? {
                "Idle" => CorpusStatus::Idle,
                "Ready" => CorpusStatus::Ready,
                tag => return Err(format!("unknown fieldless enum corpus tag `{tag}`")),
            };
            wire_roundtrip(value)
        }
        "nested_account_wire" => {
            let account = CorpusAccount {
                balance_cents: input_string(input, "balance_cents")?
                    .parse()
                    .map_err(|_| "balance_cents must be a u64 decimal string".to_string())?,
                bonus: optional_i64_string(input, "bonus")?,
                tags: input
                    .get("tags")
                    .and_then(Value::as_array)
                    .ok_or_else(|| "nested account needs a tags array".to_string())?
                    .iter()
                    .map(parse_tag)
                    .collect::<Result<Vec<_>, _>>()?,
                primary_tag: match input.get("primary_tag") {
                    Some(Value::Null) => None,
                    Some(value) => Some(parse_tag(value)?),
                    None => return Err("nested account needs primary_tag".to_string()),
                },
            };
            wire_roundtrip(account)
        }
        other => Err(format!("unknown semantic corpus operation `{other}`")),
    }
}

fn wire_roundtrip<T>(value: T) -> Result<Value, String>
where
    T: ClientDecode + ClientEncode,
{
    let wire = value.encode();
    let decoded = T::decode(&wire).map_err(|err| err.to_string())?;
    let roundtrip_wire = decoded.encode();
    let semantic: Value = serde_json::from_str(&wire)
        .map_err(|err| format!("ClientEncode emitted invalid JSON: {err}"))?;
    Ok(json!({
        "wire": wire,
        "roundtrip_wire": roundtrip_wire,
        "semantic": semantic,
    }))
}

fn parse_tag(value: &Value) -> Result<CorpusTag, String> {
    Ok(CorpusTag {
        label: input_string(value, "label")?.to_string(),
    })
}

fn optional_i64_string(input: &Value, key: &str) -> Result<Option<i64>, String> {
    match input.get(key) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => value
            .parse()
            .map(Some)
            .map_err(|_| format!("`{key}` must be an i64 decimal string or null")),
        _ => Err(format!("`{key}` must be an i64 decimal string or null")),
    }
}

fn parse_i64(input: &Value, key: &str) -> Result<i64, String> {
    input_string(input, key)?
        .parse()
        .map_err(|_| format!("`{key}` must be an i64 decimal string"))
}

fn input_string<'a>(input: &'a Value, key: &str) -> Result<&'a str, String> {
    input
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("semantic corpus input needs string `{key}`"))
}

fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("semantic corpus object is missing string `{key}`"))
}
