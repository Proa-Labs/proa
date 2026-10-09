use proa_macros::omit;
struct Product { title: String }
fn main() {
    let product = Product { title: String::new() };
    omit!(product.missing);
}
