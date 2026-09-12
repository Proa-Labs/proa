#[rsjs::client]
pub mod counter_logic {
    pub struct Counter {
        history: Vec<i32>,
    }

    pub fn value() -> i32 {
        1_i32
    }
}

fn main() {}
