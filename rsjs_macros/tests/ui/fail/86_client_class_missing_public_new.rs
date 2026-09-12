#[rsjs::client(class)]
pub mod counter_logic {
    pub struct Counter {
        count: i32,
    }

    impl Counter {
        fn new(count: i32) -> Self {
            Self { count }
        }

        pub fn value(&self) -> i32 {
            self.count
        }
    }
}

fn main() {}
