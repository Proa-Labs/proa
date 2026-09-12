// Test: String in {} doesn't implement Render

use proa_macros::html_sync;
use proa_core::Ctx;

fn main() {
    let name = String::from("Alice");

    let mut out = Vec::new();
    let mut cx = Ctx::with_buffer(&mut out);

    html_sync! {
        <div>
            {name}
        </div>
    }.render(&mut cx);
}
