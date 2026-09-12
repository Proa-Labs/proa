use proa_core::MdRenderSync;
use proa_macros::md;

fn require_sync<T>(value: T)
where
    T: MdRenderSync<Vec<u8>>,
{
    let _ = value;
}

fn main() {
    let title = "async-capable";
    require_sync(md!("# {title}\n"));
}
