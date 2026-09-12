// Test: Invalid attribute for tag (href on div)

use proa_macros::html_sync;

fn main() {
    html_sync! {
        <div href="/test">"This should fail"</div>
    };
}
