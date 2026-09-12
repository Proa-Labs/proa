// Required attribute: <bdo> without dir
use proa_macros::html_sync;

fn main() {
    html_sync! { <bdo>"Hebrew text"</bdo> };
}
