// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Experimental static-template updates for an explicitly opted-in dev build.
//! Ordinary and release macro expansions continue to write compiled literals.

use crate::{WriteBuf, WriteError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One generation of pre-escaped, compiler-validated static template runs.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct StaticManifest {
    /// Exact development protocol version.
    pub schema: u16,
    /// Templates keyed by an explicitly authored, project-unique ID.
    pub templates: BTreeMap<String, StaticTemplate>,
}

/// A template's fixed Rust/markup shape and replaceable static runs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StaticTemplate {
    /// Shape excludes only eligible static string literal contents.
    pub shape: String,
    /// Already escaped bytes, in consolidation order.
    pub slots: Vec<Vec<u8>>,
}

/// Load a single coherent template generation, falling back to compiled bytes
/// if publication is absent or incompatible. This filesystem/JSON cost is
/// confined to the explicit development macro path.
#[doc(hidden)]
pub fn load(path: &str, id: &str, shape: &str, slots: usize) -> Option<StaticTemplate> {
    // proa-perf-ok: explicit opt-in development template loading, never emitted by the ordinary/release macros
    let bytes = std::fs::read(path).ok()?;
    let mut manifest: StaticManifest = serde_json::from_slice(&bytes).ok()?;
    if manifest.schema != 1 {
        return None;
    }
    let template = manifest.templates.remove(id)?;
    (template.shape == shape && template.slots.len() == slots).then_some(template)
}

/// Write a selected development run or its original compiled fallback.
#[doc(hidden)]
pub fn write_slot<B: WriteBuf>(
    out: &mut B,
    template: Option<&StaticTemplate>,
    slot: usize,
    fallback: &'static [u8],
) -> Result<(), WriteError> {
    match template.and_then(|template| template.slots.get(slot)) {
        Some(bytes) => out.extend(bytes),
        None => out.extend_static(fallback),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_template_snapshot_remains_coherent_after_a_later_publication() {
        let path = std::env::temp_dir().join(format!(
            "proa-hot-runtime-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let make = |value: u8| StaticManifest {
            schema: 1,
            templates: BTreeMap::from([(
                "page".into(),
                StaticTemplate {
                    shape: "shape".into(),
                    slots: vec![vec![value], vec![value]],
                },
            )]),
        };
        std::fs::write(&path, serde_json::to_vec(&make(b'a')).unwrap()).unwrap();
        let snapshot = load(path.to_str().unwrap(), "page", "shape", 2).unwrap();
        std::fs::write(&path, serde_json::to_vec(&make(b'b')).unwrap()).unwrap();
        assert_eq!(snapshot.slots, vec![vec![b'a'], vec![b'a']]);
        assert_eq!(
            load(path.to_str().unwrap(), "page", "shape", 2)
                .unwrap()
                .slots,
            vec![vec![b'b'], vec![b'b']]
        );
        assert!(load(path.to_str().unwrap(), "page", "different Rust", 2).is_none());
        assert!(load(path.to_str().unwrap(), "page", "shape", 3).is_none());
        std::fs::write(&path, b"interrupted JSON").unwrap();
        assert!(load(path.to_str().unwrap(), "page", "shape", 2).is_none());
        std::fs::remove_file(path).unwrap();
    }
}
