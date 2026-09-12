// Required attribute: <img> without src or srcset
use proa_macros::html_sync;

fn main() {
    html_sync! { <img alt="photo" /> };
}
