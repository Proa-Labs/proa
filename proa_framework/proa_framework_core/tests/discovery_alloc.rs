// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use proa_core::{BasePath, WebContext};
use proa_framework_core::{Sitemap, SitemapIndex, SitemapIndexEntry};

struct ThreadCountingAllocator;

thread_local! {
    static TRACKING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

#[global_allocator]
static ALLOCATOR: ThreadCountingAllocator = ThreadCountingAllocator;

fn record_allocation() {
    if TRACKING.try_with(Cell::get).unwrap_or(false) {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
    }
}

// Delegate every allocation to System unchanged; count only the current thread.
unsafe impl GlobalAlloc for ThreadCountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record_allocation();
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record_allocation();
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

fn count_allocations<T>(run: impl FnOnce() -> T) -> (T, usize) {
    struct ResetTracking;
    impl Drop for ResetTracking {
        fn drop(&mut self) {
            TRACKING.with(|tracking| tracking.set(false));
        }
    }

    ALLOCATIONS.with(|count| count.set(0));
    TRACKING.with(|tracking| tracking.set(true));
    let reset = ResetTracking;
    let result = run();
    drop(reset);
    (result, ALLOCATIONS.with(Cell::get))
}

#[test]
fn single_page_xml_needs_no_allocations_beyond_its_output_context() {
    // The owned output page is unavoidable. Neither URL validation nor URL
    // composition should allocate again while rendering validated documents.
    let (_, context_allocations) = count_allocations(WebContext::new);
    for absolute in [false, true] {
        for count in [1, 16] {
            let origin = "https://example.com";
            let mount = "/apps/a&b";
            let path = "/search?q=a&b=<x>";
            let full_path = format!("{origin}{mount}{path}");
            let entry = if absolute { &full_path } else { path };
            let mut sitemap = Sitemap::new(origin)
                .unwrap()
                .with_base_path(BasePath::new(mount).unwrap());
            let mut index = SitemapIndex::new(origin)
                .unwrap()
                .with_base_path(BasePath::new(mount).unwrap());
            for _ in 0..count {
                sitemap = sitemap.with_url(entry).unwrap();
                index = index
                    .with_entry(SitemapIndexEntry::new(entry).unwrap())
                    .unwrap();
            }
            for is_index in [false, true] {
                let (result, allocations) = count_allocations(|| {
                    if is_index {
                        index.render_xml()
                    } else {
                        sitemap.render_xml()
                    }
                });
                let xml = result.unwrap();
                assert!(xml.len() < 4096, "fixture must fit one output page");
                assert_eq!(xml.matches("<loc>").count(), count);
                assert_eq!(
                    xml.matches(
                        "<loc>https://example.com/apps/a&amp;b/search?q=a&amp;b=&lt;x&gt;</loc>"
                    )
                    .count(),
                    count
                );
                assert_eq!(
                    allocations, context_allocations,
                    "temporary allocations: absolute={absolute}, index={is_index}, entries={count}"
                );
            }
        }
    }
}
