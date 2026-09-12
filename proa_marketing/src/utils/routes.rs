// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

pub(crate) fn collection_route_path(base_path: &'static str, path: &str) -> String {
    let path = path.trim_matches('/');
    if path.is_empty() {
        base_path.to_string()
    } else {
        format!("{base_path}/{path}")
    }
}

#[cfg(test)]
mod tests {
    use super::collection_route_path;

    #[test]
    fn collection_paths_normalize_surrounding_slashes() {
        assert_eq!(collection_route_path("/docs", ""), "/docs");
        assert_eq!(collection_route_path("/docs", "/core/"), "/docs/core");
    }
}
