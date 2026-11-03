//! A named argument the template never references is rejected, like
//! `format!`'s "named argument never used". Rust checks the actual binding.

use proa_macros::md_sync;

fn main() {
    let title = "Title";
    let _ = proa_core::render_to_string(md_sync! { r###"
        # {title}
        "###,
        extra = 1
    });
}
