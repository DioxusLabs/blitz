//! Reads the text backend feature into the `text_parley` cfg, which the code reads in its place.

fn main() {
    println!("cargo::rustc-check-cfg=cfg(text_parley)");
    if std::env::var_os("CARGO_FEATURE_PARLEY").is_some() {
        println!("cargo::rustc-cfg=text_parley");
    }
}
