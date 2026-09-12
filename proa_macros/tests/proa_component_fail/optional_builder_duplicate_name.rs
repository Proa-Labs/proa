use proa_macros::ProaComponent;

#[derive(ProaComponent)]
#[component(tag = "meter", constructor)]
struct DuplicateBuilder {
    #[component(optional_builder = "with_value")]
    low: Option<u8>,
    #[component(optional_builder = "with_value")]
    high: Option<u16>,
}

fn main() {}
