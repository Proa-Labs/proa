// a11y: role="combobox" missing required aria-controls and aria-expanded
use proa_macros::html_sync;

fn main() {
    html_sync! { <div role="combobox">"Search"</div> };
}
