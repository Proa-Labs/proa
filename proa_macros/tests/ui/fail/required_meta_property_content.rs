// Required attribute: property metadata needs content in every HTML macro.
use proa_macros::{html, html_send, html_sync};

fn main() {
    html_sync! {
        <meta property="og:title" />
    };
    html! {
        <meta property="og:title" />
    };
    html_send! {
        <meta property="og:title" />
    };
}
