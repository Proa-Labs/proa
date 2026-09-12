// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Hidden relocation transport for compiler-authored client-helper edges.
//!
//! `#[rsjs::client]` emits the defining record beside the rewritten Rust
//! helper. An analyzed call site can use [`crate::__rsjs_emit_client_helper_edge`]
//! to make rustc resolve imports and re-exports, then preserve that resolved
//! function identity as an object relocation.

/// Emit one descriptor-pass client-helper edge through rustc-resolved function
/// identity.
///
/// This is a compiler-internal prototype surface. The analyzer will eventually
/// invoke it for reachable helper calls after assigning the source edge ID.
/// The helper's source spelling is never encoded as identity.
#[doc(hidden)]
#[macro_export]
macro_rules! __rsjs_emit_client_helper_edge {
    (
        $record_ident:ident,
        $cargo_unit_id:expr,
        $source_edge_id:expr,
        $helper:path
    ) => {
        #[doc(hidden)]
        #[allow(non_upper_case_globals)]
        const _: () = {
            #[used]
            #[cfg_attr(target_vendor = "apple", unsafe(link_section = "__DATA,__rsjs_hedge"))]
            #[cfg_attr(
                any(
                    target_os = "linux",
                    target_os = "android",
                    target_os = "freebsd",
                    target_os = "netbsd",
                    target_os = "openbsd",
                    target_os = "dragonfly",
                    target_os = "solaris",
                    target_os = "illumos",
                    target_os = "haiku",
                    target_os = "redox"
                ),
                unsafe(link_section = ".rsjs.helper_edge")
            )]
            #[cfg_attr(target_os = "windows", unsafe(link_section = ".rsjs$I"))]
            static $record_ident: $crate::__private::HelperEdgeRecord =
                $crate::__private::HelperEdgeRecord::new(
                    $cargo_unit_id,
                    $source_edge_id,
                    $helper as *const (),
                );

            #[cfg(not(any(
                target_vendor = "apple",
                target_os = "linux",
                target_os = "android",
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "openbsd",
                target_os = "dragonfly",
                target_os = "solaris",
                target_os = "illumos",
                target_os = "haiku",
                target_os = "redox",
                target_os = "windows"
            )))]
            compile_error!(
                "RSJS descriptor relocation transport does not support this target object format"
            );
        };
    };
    ($record_ident:ident, $source_edge_id:expr, $helper:path) => {
        $crate::__rsjs_emit_client_helper_edge!(
            $record_ident,
            $crate::__private::UNASSIGNED_CARGO_UNIT_ID,
            $source_edge_id,
            $helper
        );
    };
}
