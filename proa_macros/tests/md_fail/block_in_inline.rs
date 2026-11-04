use proa_core::MdRenderSync;
use proa_macros::md_sync;

fn main() {
    let mut cx = proa_core::WebContext::new();
    md_sync! { "Label: {md_sync! { r#\"## Heading\n\"# }}" }
        .render_md(&mut cx)
        .unwrap();
}
