// Nesting: <caption> not first in <table>
use proa_macros::html_sync;

fn main() {
    html_sync! { <table><tr><td>"data"</td></tr><caption>"bad"</caption></table> };
}
