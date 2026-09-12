// a11y: abstract ARIA role used directly
use proa_macros::html_sync;

fn main() {
    html_sync! { <div role="widget">"thing"</div> };
}
