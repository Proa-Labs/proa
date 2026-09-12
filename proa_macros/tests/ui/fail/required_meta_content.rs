// Required attribute: <meta name="..."> without content
use proa_macros::html_sync;

fn main() {
    html_sync! { <meta name="description" /> };
}
