#[rsjs::client(class)]
pub mod counter_logic {
    pub struct Counter {
        count: i32,
    }

    impl Counter {
        pub fn new(count: i32) -> Self {
            Self { count }
        }
    }

    impl Drop for Counter {
        fn drop(&mut self) {
            self.count = 0_i32;
        }
    }
}

fn main() {}
