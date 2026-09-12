use proa_macros::html_sync;

fn main() {
    let scheme = "java";
    let _ = html_sync! {
        <a href={text!("{}script:alert(1)", scheme)}>"unsafe"</a>
    };
}
