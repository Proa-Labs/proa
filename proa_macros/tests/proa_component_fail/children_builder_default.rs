use proa_macros::ProaComponent;

#[derive(ProaComponent)]
#[component(tag = "section", constructor, children_builder)]
struct DefaultedChild<C> {
    #[component(children, default)]
    children: Option<C>,
}

fn main() {}
