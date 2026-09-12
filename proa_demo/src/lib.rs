// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Proa Demo FFI — exposes showcase renders as C ABI entry points.
//!
//! Build targets:
//!   Native:  cargo build -p proa_demo --release
//!   WASM:    cargo build -p proa_demo --release --target wasm32-unknown-unknown

#[cfg(feature = "server")]
extern crate self as proa_demo;

pub mod ebay_srp;
pub mod ebay_srp_chrome;
pub mod ebay_srp_chrome_real;
pub mod ebay_srp_filter_runtime;
pub mod ebay_srp_islands;
pub mod github_pr_diff;
pub mod json_types;
pub mod ops_dashboard;
#[cfg(feature = "server")]
#[allow(dead_code)]
pub mod playground;
pub mod rsjs_query_ide_probe;
pub mod settings_console;
pub mod showcase_index;
pub mod streaming_airbnb_home;

use proa_core::WebContext;
use proa_showcase::airbnb;
use proa_showcase::nike;

// ---------------------------------------------------------------------------
// Allocating variants — return a freshly allocated buffer via out-params
// ---------------------------------------------------------------------------

/// Render the Airbnb homepage. Returns 0 on success.
///
/// # Safety
/// `out_ptr` and `out_len` must be valid, writable pointers.
#[no_mangle]
pub unsafe extern "C" fn render_airbnb_home(out_ptr: *mut *mut u8, out_len: *mut usize) -> i32 {
    render_into_out(airbnb::home::render(), out_ptr, out_len)
}

/// Render the Airbnb search page. Returns 0 on success.
///
/// # Safety
/// `out_ptr` and `out_len` must be valid, writable pointers.
#[no_mangle]
pub unsafe extern "C" fn render_airbnb_search(out_ptr: *mut *mut u8, out_len: *mut usize) -> i32 {
    render_into_out(airbnb::search::render(), out_ptr, out_len)
}

/// Render the Airbnb listing page with default (Malibu) data. Returns 0 on success.
///
/// # Safety
/// `out_ptr` and `out_len` must be valid, writable pointers.
#[no_mangle]
pub unsafe extern "C" fn render_airbnb_listing(out_ptr: *mut *mut u8, out_len: *mut usize) -> i32 {
    render_into_out(airbnb::listing::render(), out_ptr, out_len)
}

/// Render the Airbnb listing page with custom JSON props. Returns 0 on success,
/// -2 if the JSON is invalid.
///
/// The JSON must conform to the `ListingData` schema (see showcase-props.json).
///
/// # Safety
/// - `json_ptr` must point to at least `json_len` valid UTF-8 bytes.
/// - `out_ptr` and `out_len` must be valid, writable pointers.
/// - Caller must eventually call `proa_free(out_ptr, out_len)` to deallocate.
#[no_mangle]
pub unsafe extern "C" fn render_airbnb_listing_json(
    json_ptr: *const u8,
    json_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    let json_bytes = unsafe { std::slice::from_raw_parts(json_ptr, json_len) };
    let json_str = match std::str::from_utf8(json_bytes) {
        Ok(s) => s,
        Err(_) => return -2,
    };
    let owned: json_types::ListingDataOwned = match serde_json::from_str(json_str) {
        Ok(d) => d,
        Err(_) => return -2,
    };

    let borrowed = owned.borrow_collections();
    let data = owned.to_listing_data(&borrowed);

    let mut buf = Vec::new();
    let mut cx = WebContext::with_buffer(&mut buf);
    let _ = airbnb::listing::render_into(&mut cx, &data, None);
    let html = String::from_utf8(buf).unwrap_or_default();

    unsafe { render_into_out(html, out_ptr, out_len) }
}

// ---------------------------------------------------------------------------
// Nike showcase — allocating variants
// ---------------------------------------------------------------------------

/// Render the Nike catalog page (120 product cards). Returns 0 on success.
///
/// # Safety
/// `out_ptr` and `out_len` must be valid, writable pointers.
#[no_mangle]
pub unsafe extern "C" fn render_nike_catalog(out_ptr: *mut *mut u8, out_len: *mut usize) -> i32 {
    render_into_out(nike::catalog::render(), out_ptr, out_len)
}

/// Render the Nike PDP (Alphafly 3). Returns 0 on success.
///
/// # Safety
/// `out_ptr` and `out_len` must be valid, writable pointers.
#[no_mangle]
pub unsafe extern "C" fn render_nike_pdp(out_ptr: *mut *mut u8, out_len: *mut usize) -> i32 {
    render_into_out(nike::product::render(), out_ptr, out_len)
}

/// Render the Nike PDP with custom JSON props. Returns 0 on success, -2 if JSON invalid.
///
/// # Safety
/// - `json_ptr` must point to at least `json_len` valid UTF-8 bytes.
/// - `out_ptr` and `out_len` must be valid, writable pointers.
/// - Caller must call `proa_free(out_ptr, out_len)` to deallocate.
#[no_mangle]
pub unsafe extern "C" fn render_nike_pdp_json(
    json_ptr: *const u8,
    json_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    let json_bytes = unsafe { std::slice::from_raw_parts(json_ptr, json_len) };
    let json_str = match std::str::from_utf8(json_bytes) {
        Ok(s) => s,
        Err(_) => return -2,
    };
    let owned: json_types::NikePdpDataOwned = match serde_json::from_str(json_str) {
        Ok(d) => d,
        Err(_) => return -2,
    };

    let borrowed = owned.borrow_collections();
    let data = owned.to_product_data(&borrowed);

    let mut buf = Vec::new();
    let mut cx = WebContext::with_buffer(&mut buf);
    nike::product::render_into(&mut cx, &data);
    let html = String::from_utf8(buf).unwrap_or_default();

    unsafe { render_into_out(html, out_ptr, out_len) }
}

/// Render the Nike catalog with custom JSON props. Returns 0 on success, -2 if JSON invalid.
///
/// # Safety
/// - `json_ptr` must point to at least `json_len` valid UTF-8 bytes.
/// - `out_ptr` and `out_len` must be valid, writable pointers.
/// - Caller must call `proa_free(out_ptr, out_len)` to deallocate.
#[no_mangle]
pub unsafe extern "C" fn render_nike_catalog_json(
    json_ptr: *const u8,
    json_len: usize,
    out_ptr: *mut *mut u8,
    out_len: *mut usize,
) -> i32 {
    let json_bytes = unsafe { std::slice::from_raw_parts(json_ptr, json_len) };
    let json_str = match std::str::from_utf8(json_bytes) {
        Ok(s) => s,
        Err(_) => return -2,
    };
    let owned: json_types::NikeCatalogDataOwned = match serde_json::from_str(json_str) {
        Ok(d) => d,
        Err(_) => return -2,
    };

    let borrowed = owned.borrow_collections();
    let products = owned.build_products(&borrowed);
    let data = nike::catalog::CatalogData {
        title: &owned.title,
        products: &products,
    };

    let mut buf = Vec::new();
    let mut cx = WebContext::with_buffer(&mut buf);
    nike::catalog::render_into(&mut cx, &data);
    let html = String::from_utf8(buf).unwrap_or_default();

    unsafe { render_into_out(html, out_ptr, out_len) }
}

// ---------------------------------------------------------------------------
// Nike showcase — buffer-reuse variants
// ---------------------------------------------------------------------------

/// Render the Nike catalog into a caller-provided buffer.
///
/// # Safety
/// See `render_airbnb_home_into`.
#[no_mangle]
pub unsafe extern "C" fn render_nike_catalog_into(
    buf_ptr: *mut u8,
    buf_cap: usize,
    out_len: *mut usize,
) -> i32 {
    render_into_buf(nike::catalog::render(), buf_ptr, buf_cap, out_len)
}

/// Render the Nike product detail into a caller-provided buffer.
///
/// # Safety
/// See `render_airbnb_home_into`.
#[no_mangle]
pub unsafe extern "C" fn render_nike_pdp_into(
    buf_ptr: *mut u8,
    buf_cap: usize,
    out_len: *mut usize,
) -> i32 {
    render_into_buf(nike::product::render(), buf_ptr, buf_cap, out_len)
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Convert a `String` into a `(ptr, len)` pair returned through out-params.
/// Uses `into_boxed_slice` to ensure len == capacity for safe deallocation.
///
/// # Safety
/// Caller must eventually call `proa_free(ptr, len)` to deallocate.
unsafe fn render_into_out(html: String, out_ptr: *mut *mut u8, out_len: *mut usize) -> i32 {
    let boxed = html.into_bytes().into_boxed_slice();
    let len = boxed.len();
    let ptr = Box::into_raw(boxed) as *mut u8;
    unsafe {
        *out_ptr = ptr;
        *out_len = len;
    }
    0
}

/// Free a buffer previously returned by a `render_airbnb_*` function.
///
/// # Safety
/// `ptr` and `len` must match a previous allocation from this library.
#[no_mangle]
pub unsafe extern "C" fn proa_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() && len > 0 {
        unsafe {
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, len)));
        }
    }
}

// ---------------------------------------------------------------------------
// Buffer-reuse variants — write into caller-provided buffer, zero alloc
// ---------------------------------------------------------------------------

/// Render the Airbnb homepage into a caller-provided buffer.
///
/// Returns 0 on success, -1 if the buffer is too small.
/// `out_len` receives the number of bytes written.
///
/// # Safety
/// `buf_ptr` must point to at least `buf_cap` writable bytes.
/// `out_len` must be a valid, writable pointer.
#[no_mangle]
pub unsafe extern "C" fn render_airbnb_home_into(
    buf_ptr: *mut u8,
    buf_cap: usize,
    out_len: *mut usize,
) -> i32 {
    render_into_buf(airbnb::home::render(), buf_ptr, buf_cap, out_len)
}

/// Render the Airbnb search page into a caller-provided buffer.
///
/// # Safety
/// See `render_airbnb_home_into`.
#[no_mangle]
pub unsafe extern "C" fn render_airbnb_search_into(
    buf_ptr: *mut u8,
    buf_cap: usize,
    out_len: *mut usize,
) -> i32 {
    render_into_buf(airbnb::search::render(), buf_ptr, buf_cap, out_len)
}

/// Render the Airbnb listing page into a caller-provided buffer.
///
/// # Safety
/// See `render_airbnb_home_into`.
#[no_mangle]
pub unsafe extern "C" fn render_airbnb_listing_into(
    buf_ptr: *mut u8,
    buf_cap: usize,
    out_len: *mut usize,
) -> i32 {
    render_into_buf(airbnb::listing::render(), buf_ptr, buf_cap, out_len)
}

/// Write rendered HTML into a caller-provided buffer.
/// Returns 0 on success, -1 if buffer too small.
unsafe fn render_into_buf(
    html: String,
    buf_ptr: *mut u8,
    buf_cap: usize,
    out_len: *mut usize,
) -> i32 {
    let bytes = html.as_bytes();
    if bytes.len() > buf_cap {
        unsafe {
            *out_len = bytes.len();
        }
        return -1;
    }
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf_ptr, bytes.len());
        *out_len = bytes.len();
    }
    0
}

// ---------------------------------------------------------------------------
// WASM-specific exports
// ---------------------------------------------------------------------------

/// WASM allocator — allocate `size` bytes and return the pointer.
#[no_mangle]
pub extern "C" fn wasm_alloc(size: usize) -> *mut u8 {
    let mut buf = Vec::with_capacity(size);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// WASM deallocator — free a buffer allocated by `wasm_alloc`.
///
/// # Safety
/// `ptr` must have been allocated by `wasm_alloc` with the given `size`.
#[no_mangle]
pub unsafe extern "C" fn wasm_dealloc(ptr: *mut u8, size: usize) {
    if !ptr.is_null() && size > 0 {
        unsafe {
            drop(Vec::from_raw_parts(ptr, 0, size));
        }
    }
}
