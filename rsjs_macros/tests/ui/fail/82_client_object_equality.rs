#[rsjs::client]
pub mod counter_logic {
    #[derive(PartialEq)]
    pub struct Counter {
        count: i32,
    }

    pub fn equal() -> bool {
        let left = Counter { count: 1_i32 };
        let right = Counter { count: 1_i32 };
        left == right
    }
}

fn main() {}
