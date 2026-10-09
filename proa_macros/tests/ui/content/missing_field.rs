use proa_macros::omit_content;
struct Product { title: String }
fn main() {
    let product = Product { title: String::new() };
    omit_content!(product.missing);
}
