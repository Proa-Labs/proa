// Nesting: block element <div> inside phrasing-only <span>
use proa_macros::html_sync;

fn main() {
    html_sync! { <span><div>"bad"</div></span> };
}
