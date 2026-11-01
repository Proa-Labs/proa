use proa_macros::md_sync;

fn main() {
    let _ = md_sync!("{if false {A} elsse {B}}");
    let _ = md_sync!("{for x in [1] {X} ignored}");
    let _ = md_sync!("{if true {\nif false {A} elsse {B}\n}}");
}
