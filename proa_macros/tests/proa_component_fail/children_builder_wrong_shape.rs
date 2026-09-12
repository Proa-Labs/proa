use proa_macros::ProaComponent;

#[derive(ProaComponent)]
#[component(tag = "section", children_builder)]
struct ConcreteChildren {
    #[component(children)]
    children: Vec<&'static str>,
}

fn main() {}
