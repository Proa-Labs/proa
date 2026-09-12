#![allow(unused_imports)]

use rsjs_macros::rsjs;

#[rsjs::client(class)]
pub mod colliding_names {
    pub struct FooBar {
        value: i32,
    }

    impl FooBar {
        pub fn new(value: i32) -> Self {
            Self { value }
        }

        pub fn value(&self) -> i32 {
            self.value
        }
    }

    pub struct Foo_Bar {
        value: i32,
    }

    impl Foo_Bar {
        pub fn new(value: i32) -> Self {
            Self { value }
        }

        pub fn value(&self) -> i32 {
            self.value
        }
    }
}

fn main() {}
