#[rsjs::client]
pub mod counter_logic {
    pub struct Counter {
        count: i32,
    }

    impl Counter {
        pub fn into_value(self) -> i32 {
            self.count
        }
    }

    pub fn value() -> i32 {
        Counter { count: 1_i32 }.into_value()
    }
}

fn main() {}
