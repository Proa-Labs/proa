// Category 56: `#[derive(SharedState)]` rejects encode-only (borrowed)
// fields with a field-pointed round-trip explanation instead of deferring
// to an unguided E0277 on the generated `impl SharedState`.

#[derive(rsjs::SharedState)]
#[rsjs(type_key = "probe.borrowed")]
pub struct Borrowed {
    pub label: &'static str,
}

fn main() {}
