/*
 * Proa SSR Engine — C FFI Interface
 *
 * Build:
 *   cargo build -p proa_demo --release
 *
 * Output:
 *   target/release/libproa_demo.dylib   (macOS)
 *   target/release/libproa_demo.so      (Linux)
 *   target/release/proa_demo.dll        (Windows)
 */

#ifndef PROA_H
#define PROA_H

#include <stddef.h>
#include <stdint.h>

/**
 * Render the Airbnb homepage. Returns 0 on success.
 *
 * # Safety
 * `out_ptr` and `out_len` must be valid, writable pointers.
 */
int32_t render_airbnb_home(uint8_t **out_ptr, uintptr_t *out_len);

/**
 * Render the Airbnb search page. Returns 0 on success.
 *
 * # Safety
 * `out_ptr` and `out_len` must be valid, writable pointers.
 */
int32_t render_airbnb_search(uint8_t **out_ptr, uintptr_t *out_len);

/**
 * Render the Airbnb listing page with default (Malibu) data. Returns 0 on success.
 *
 * # Safety
 * `out_ptr` and `out_len` must be valid, writable pointers.
 */
int32_t render_airbnb_listing(uint8_t **out_ptr, uintptr_t *out_len);

/**
 * Render the Airbnb listing page with custom JSON props. Returns 0 on success,
 * -2 if the JSON is invalid.
 *
 * The JSON must conform to the `ListingData` schema (see showcase-props.json).
 *
 * # Safety
 * - `json_ptr` must point to at least `json_len` valid UTF-8 bytes.
 * - `out_ptr` and `out_len` must be valid, writable pointers.
 * - Caller must eventually call `proa_free(out_ptr, out_len)` to deallocate.
 */
int32_t render_airbnb_listing_json(const uint8_t *json_ptr,
                                   uintptr_t json_len,
                                   uint8_t **out_ptr,
                                   uintptr_t *out_len);

/**
 * Render the Nike catalog page (120 product cards). Returns 0 on success.
 *
 * # Safety
 * `out_ptr` and `out_len` must be valid, writable pointers.
 */
int32_t render_nike_catalog(uint8_t **out_ptr, uintptr_t *out_len);

/**
 * Render the Nike PDP (Alphafly 3). Returns 0 on success.
 *
 * # Safety
 * `out_ptr` and `out_len` must be valid, writable pointers.
 */
int32_t render_nike_pdp(uint8_t **out_ptr, uintptr_t *out_len);

/**
 * Render the Nike PDP with custom JSON props. Returns 0 on success, -2 if JSON invalid.
 *
 * # Safety
 * - `json_ptr` must point to at least `json_len` valid UTF-8 bytes.
 * - `out_ptr` and `out_len` must be valid, writable pointers.
 * - Caller must call `proa_free(out_ptr, out_len)` to deallocate.
 */
int32_t render_nike_pdp_json(const uint8_t *json_ptr,
                             uintptr_t json_len,
                             uint8_t **out_ptr,
                             uintptr_t *out_len);

/**
 * Render the Nike catalog with custom JSON props. Returns 0 on success, -2 if JSON invalid.
 *
 * # Safety
 * - `json_ptr` must point to at least `json_len` valid UTF-8 bytes.
 * - `out_ptr` and `out_len` must be valid, writable pointers.
 * - Caller must call `proa_free(out_ptr, out_len)` to deallocate.
 */
int32_t render_nike_catalog_json(const uint8_t *json_ptr,
                                 uintptr_t json_len,
                                 uint8_t **out_ptr,
                                 uintptr_t *out_len);

/**
 * Render the Nike catalog into a caller-provided buffer.
 *
 * # Safety
 * See `render_airbnb_home_into`.
 */
int32_t render_nike_catalog_into(uint8_t *buf_ptr, uintptr_t buf_cap, uintptr_t *out_len);

/**
 * Render the Nike PDP into a caller-provided buffer.
 *
 * # Safety
 * See `render_airbnb_home_into`.
 */
int32_t render_nike_pdp_into(uint8_t *buf_ptr, uintptr_t buf_cap, uintptr_t *out_len);

/**
 * Free a buffer previously returned by a `render_airbnb_*` function.
 *
 * # Safety
 * `ptr` and `len` must match a previous allocation from this library.
 */
void proa_free(uint8_t *ptr, uintptr_t len);

/**
 * Render the Airbnb homepage into a caller-provided buffer.
 *
 * Returns 0 on success, -1 if the buffer is too small.
 * `out_len` receives the number of bytes written.
 *
 * # Safety
 * `buf_ptr` must point to at least `buf_cap` writable bytes.
 * `out_len` must be a valid, writable pointer.
 */
int32_t render_airbnb_home_into(uint8_t *buf_ptr, uintptr_t buf_cap, uintptr_t *out_len);

/**
 * Render the Airbnb search page into a caller-provided buffer.
 *
 * # Safety
 * See `render_airbnb_home_into`.
 */
int32_t render_airbnb_search_into(uint8_t *buf_ptr, uintptr_t buf_cap, uintptr_t *out_len);

/**
 * Render the Airbnb listing page into a caller-provided buffer.
 *
 * # Safety
 * See `render_airbnb_home_into`.
 */
int32_t render_airbnb_listing_into(uint8_t *buf_ptr, uintptr_t buf_cap, uintptr_t *out_len);

/**
 * WASM allocator — allocate `size` bytes and return the pointer.
 */
uint8_t *wasm_alloc(uintptr_t size);

/**
 * WASM deallocator — free a buffer allocated by `wasm_alloc`.
 *
 * # Safety
 * `ptr` must have been allocated by `wasm_alloc` with the given `size`.
 */
void wasm_dealloc(uint8_t *ptr, uintptr_t size);

#endif  /* PROA_H */
