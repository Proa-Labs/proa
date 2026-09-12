// Required attribute: <input type="image"> without alt
use proa_macros::html_sync;

fn main() {
    html_sync! { <input type="image" src="/btn.png" /> };
}
