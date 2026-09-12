use proa_macros::ProaComponent;

#[derive(ProaComponent)]
#[component(tag = "progress")]
struct MissingConstructor {
    #[component(default)]
    value: usize,
}

fn main() {}
