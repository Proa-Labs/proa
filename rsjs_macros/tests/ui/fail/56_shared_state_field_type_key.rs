// Category 56: `#[derive(SharedState)]` rejects a `type_key` written on a
// field, pointing at the misplaced option instead of claiming the protocol
// identity is missing.

#[derive(rsjs::SharedState)]
pub struct FieldKey {
    #[rsjs(type_key = "probe.fieldkey")]
    pub mode: bool,
}

fn main() {}
