use proa_core::MdRenderSync;
use proa_macros::md_sync;

fn main() {
    let mut cx = proa_core::WebContext::new();
    md_sync! { r###"
        [{::proa_macros::md_sync! { r#"
            ## Heading
            "# }}](/target)
        "###
    }
    .render_md(&mut cx)
    .unwrap();
}
