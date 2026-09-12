// Category 56: `#[derive(SharedState)]` rejects enums and points at the
// field-codec alternative.

#[derive(rsjs::SharedState)]
#[rsjs(type_key = "probe.enum")]
pub enum Mode {
    On,
    Off,
}

fn main() {}
