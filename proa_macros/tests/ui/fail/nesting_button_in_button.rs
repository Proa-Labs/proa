// Nesting: <button> inside <button>
use proa_macros::html_sync;

fn main() {
    html_sync! { <button><button>"bad"</button></button> };
}
