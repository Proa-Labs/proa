// Nesting: block element <div> inside phrasing-only <p>
use proa_macros::html_sync;

fn main() {
    html_sync! { <p><div>"bad"</div></p> };
}
