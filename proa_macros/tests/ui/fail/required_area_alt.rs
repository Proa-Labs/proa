// Required attribute: <area href> without alt
use proa_macros::html_sync;

fn main() {
    html_sync! { <area href="/page" /> };
}
