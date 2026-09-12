use proa_macros::ProaComponent;

#[derive(ProaComponent)]
#[component(tag = "label", constructor)]
struct WrongShape {
    #[component(optional_builder = "with_text")]
    text: &'static str,
}

fn main() {}
