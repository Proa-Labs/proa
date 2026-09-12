// Required attribute: <meta> without name/http-equiv/charset/itemprop
use proa_macros::html_sync;

fn main() {
    html_sync! { <meta content="foo" /> };
}
