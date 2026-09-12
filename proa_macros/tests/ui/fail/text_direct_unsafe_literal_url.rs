use proa_macros::html_sync;

fn main() {
    let _ = html_sync! {
        <a href={text!("javascript:alert(1)")}>"unsafe"</a>
    };
}
