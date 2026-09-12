// Nesting: <form> inside <form>
use proa_macros::html_sync;

fn main() {
    html_sync! { <form action="/a"><div><form action="/b">"bad"</form></div></form> };
}
