// Category 1: `#[rsjs(component)]` requires a real render-trait impl.

use rsjs::rsjs;

pub struct Counter;

#[rsjs(component)]
impl Counter {
    fn render(&self) {}
}

fn main() {}
