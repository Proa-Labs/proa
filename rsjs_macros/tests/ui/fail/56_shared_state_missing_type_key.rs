// Category 56: `#[derive(SharedState)]` requires a struct-level protocol
// identity and says exactly how to add one.

#[derive(rsjs::SharedState)]
pub struct MissingKey {
    pub mode: bool,
}

fn main() {}
