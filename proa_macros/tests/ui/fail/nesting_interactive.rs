// Nesting: interactive <a> inside interactive <button>
use proa_macros::html_sync;

fn main() {
    html_sync! { <button><a href="/link">"bad"</a></button> };
}
