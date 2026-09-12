//! Binding the same name twice is rejected.

use proa_macros::md_sync;

fn main() {
    let _ = md_sync! { r###"
        {value}
        "###,
        value = 1,
        value = 2
    };
}
