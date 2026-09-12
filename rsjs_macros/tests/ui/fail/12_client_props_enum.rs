// Category 12: `#[derive(ClientProps)]` rejects data-carrying enums.

use rsjs::ClientProps;

#[derive(ClientProps)]
pub enum Mode {
    On,
    Custom(String),
}

fn main() {}
