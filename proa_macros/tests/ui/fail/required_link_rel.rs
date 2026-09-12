// Required attribute: <link> without rel
use proa_macros::html_sync;

fn main() {
    html_sync! { <link href="/style.css" /> };
}
