// Required attribute: property metadata needs content.
use proa_macros::html_sync;

fn main() {
    html_sync! {
        <meta property="og:title" />
    };
}
