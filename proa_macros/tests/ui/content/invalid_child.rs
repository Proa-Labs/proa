use proa_macros::omit_content;
struct Notice<T>(T);
fn main() {
    omit_content!(child Notice<String>);
}
