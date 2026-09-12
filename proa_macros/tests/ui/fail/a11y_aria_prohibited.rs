// a11y: ARIA on element that prohibits it
use proa_macros::html_sync;

fn main() {
    html_sync! { <meta name="x" content="y" role="none" /> };
}
