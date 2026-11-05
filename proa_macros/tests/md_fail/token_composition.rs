use proa_macros::{md, md_sync};

fn main() {
    let overview = ();
    let impact = ();
    proa_core::render_to_string(md_sync! { {overview}{impact} });
    proa_core::render_to_string(md_sync! { "**" {overview} "**" });
    proa_core::render_to_string(md_sync! {});
    let _ = proa_core::render_md_to_string_async(md! { {impact} });
}
