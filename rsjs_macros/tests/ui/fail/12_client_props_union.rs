// Category 12: `#[derive(ClientProps)]` rejects unions.

use rsjs::ClientProps;

#[derive(ClientProps)]
pub union Bits {
    a: u32,
    b: f32,
}

fn main() {}
