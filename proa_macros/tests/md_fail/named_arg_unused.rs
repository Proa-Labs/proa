//! A named argument the template never references is rejected, like
//! `format!`'s "named argument never used".

use proa_macros::md_sync;

fn main() {
    let title = "Title";
    let _ = md_sync! { r###"
        # {title}
        "###,
        extra = 1
    };
}
