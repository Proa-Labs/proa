// Test: Invalid attribute syntax

use proa_macros::html_sync;

fn main() {
    html_sync! {
        <div class>
            "Content"
        </div>
    };
}
