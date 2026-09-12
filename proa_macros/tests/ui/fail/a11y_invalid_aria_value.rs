// a11y: invalid enumerated ARIA value
use proa_macros::html_sync;

fn main() {
    html_sync! { <div aria_hidden="yes">"hidden"</div> };
}
