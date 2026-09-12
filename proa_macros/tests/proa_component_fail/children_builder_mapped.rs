use proa_macros::ProaComponent;

#[derive(ProaComponent)]
#[component(tag = "section", children_builder)]
struct MappedChildren<C = ()> {
    #[component(children, map = "as_ref")]
    children: Option<C>,
}

fn main() {}
