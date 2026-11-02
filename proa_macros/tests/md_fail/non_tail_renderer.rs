use proa_macros::md_sync;

fn main() {
    let _ = proa_core::render_to_string(md_sync! { r#"{if true {
        md_sync! { "A" }
        md_sync! { "B" }
    }}"# });
}
