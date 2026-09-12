#![allow(unexpected_cfgs)]
#![allow(unused_imports)]

use rsjs::rsjs;

#[rsjs::client_module]
pub mod rust_string_semantics {
    #[rsjs::client]
    pub fn has_bytes(label: String) -> bool {
        label.len() > 0
    }

    #[rsjs::client]
    pub fn trim(label: String) -> String {
        label.trim().to_string()
    }
}

fn main() {
    assert!(rust_string_semantics::has_bytes("한😊".to_string()));
    assert!(!rust_string_semantics::has_bytes(String::new()));
    assert_eq!(
        rust_string_semantics::trim("\u{0085}ready\u{0085}".to_string()),
        "ready"
    );
    assert_eq!(
        rust_string_semantics::trim("\u{feff}ready\u{feff}".to_string()),
        "\u{feff}ready\u{feff}"
    );
}
