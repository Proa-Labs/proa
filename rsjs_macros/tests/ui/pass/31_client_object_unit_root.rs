#[rsjs::client]
pub mod counter_logic {
    pub struct Counter {
        count: i32,
    }

    impl Counter {
        pub fn new(count: i32) -> Self {
            Self { count }
        }

        pub fn increment(&mut self) {
            self.count += 1_i32;
        }
    }

    pub fn increment_once() {
        let mut counter = Counter::new(0_i32);
        counter.increment();
    }
}

fn main() {}
