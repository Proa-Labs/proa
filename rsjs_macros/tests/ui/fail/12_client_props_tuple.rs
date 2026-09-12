// Category 12: `#[derive(ClientProps)]` rejects tuple structs.

use rsjs::ClientProps;

#[derive(ClientProps)]
pub struct Position(i64, i64);

fn main() {}
