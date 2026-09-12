// OWASP: executable data: URL in a case-variant HTML URL attribute.
use proa_macros::html_sync;

fn main() {
    html_sync! { <a HREF="data:text/html;base64,PHNjcmlwdD4=">"bad"</a> };
}
