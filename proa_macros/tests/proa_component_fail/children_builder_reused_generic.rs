use proa_macros::ProaComponent;

#[derive(ProaComponent)]
#[component(tag = "section", children_builder)]
struct ReusedChild<C = ()> {
    fallback: Option<C>,
    #[component(children)]
    children: Option<C>,
}

fn main() {}
