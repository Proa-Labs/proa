// Test: Unclosed HTML tag should give clear error

use proa_macros::html_sync;

fn main() {
    html_sync! {
        <div>
            <h1>"Title"
    };
}
