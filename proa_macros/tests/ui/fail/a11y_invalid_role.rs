// a11y: invalid ARIA role
use proa_macros::html_sync;

fn main() {
    html_sync! { <div role="buttn">"Click"</div> };
}
