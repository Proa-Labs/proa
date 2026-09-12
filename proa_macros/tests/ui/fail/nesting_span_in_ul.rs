// Nesting: <span> is not a valid child of <ul>
use proa_macros::html_sync;

fn main() {
    html_sync! { <ul><span>"bad"</span></ul> };
}
