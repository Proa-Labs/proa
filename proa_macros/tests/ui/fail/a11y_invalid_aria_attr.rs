// a11y: invalid ARIA attribute name (typo)
use proa_macros::html_sync;

fn main() {
    html_sync! { <div aria_labeledby="title">"content"</div> };
}
