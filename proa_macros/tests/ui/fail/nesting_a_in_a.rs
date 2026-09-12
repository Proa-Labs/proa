// Nesting: <a> inside <a>
use proa_macros::html_sync;

fn main() {
    html_sync! { <a href="/outer"><a href="/inner">"bad"</a></a> };
}
