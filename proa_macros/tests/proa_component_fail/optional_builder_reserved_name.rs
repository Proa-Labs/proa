use proa_macros::ProaComponent;

#[derive(ProaComponent)]
#[component(tag = "output", constructor)]
struct ConstructorCollision {
    #[component(optional_builder = "new")]
    value: Option<u8>,
}

fn main() {}
