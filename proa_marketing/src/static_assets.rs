// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

// Static asset serving is currently wired from `main.rs` so runtime paths can
// come from `STATIC_DIR` in Docker. Keep this module as the ownership boundary
// for future cache policy and manifest helpers.
