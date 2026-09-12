#[rsjs::client(class)]
pub mod counter_logic {
    pub struct Counter {
        count: i64,
    }

    impl Counter {
        pub fn new() -> Self {
            Self { count: 0 }
        }

        pub fn increment(&mut self) {
            self.count += 1_i64;
        }
    }
}

fn main() {}
