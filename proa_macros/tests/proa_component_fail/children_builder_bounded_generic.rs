use proa_macros::ProaComponent;

#[derive(ProaComponent)]
#[component(tag = "section", children_builder)]
struct BoundedChildren<C: Default = ()> {
    #[component(children)]
    children: Option<C>,
}

fn main() {}
