// Category 12: `#[derive(ClientProps)]` rejects `Option<Option<T>>`.

use rsjs::ClientProps;

#[derive(ClientProps)]
pub struct Profile {
    pub middle_name: Option<Option<String>>,
}

fn main() {}
