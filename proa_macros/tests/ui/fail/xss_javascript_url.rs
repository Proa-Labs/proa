// OWASP: javascript: URL scheme in href
use proa_macros::html_sync;

fn main() {
    html_sync! { <a href="javascript:alert(1)">"bad"</a> };
}
