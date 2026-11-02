//! Slots are Rust. A single word inside a branch is an unresolved Rust name;
//! prose that is not Rust, trailing tokens, and misspelled branches are parse
//! errors whose message names the nested-template form.

use proa_macros::md_sync;

fn main() {
    let ready = true;
    let _ = md_sync!("{if ready {Available} else {Unavailable}}");
    let _ = md_sync!("{for x in [1] { \"X\" } ignored}");
    let _ = md_sync!("{if ready { \"ok\" } elsse { \"no\" }}");
}
