// a11y: role="checkbox" missing required aria-checked
use proa_macros::html_sync;

fn main() {
    html_sync! { <div role="checkbox">"Accept terms"</div> };
}
