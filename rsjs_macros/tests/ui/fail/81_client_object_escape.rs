#[rsjs::client]
pub mod counter_logic {
    pub struct Counter {
        count: i32,
    }

    fn consume(_counter: Counter) -> i32 {
        0_i32
    }

    pub fn value() -> i32 {
        let counter = Counter { count: 1_i32 };
        consume(counter)
    }
}

fn main() {}
