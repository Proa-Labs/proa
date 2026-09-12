// Test: href is not valid for img

use proa_macros::html_sync;

fn main() {
    html_sync! {
        <img src="/test.jpg" href="/link" />
    };
}
