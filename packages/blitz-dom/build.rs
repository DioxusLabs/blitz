//! Reads the text backend into the `text_parley` cfg, which the code reads in its place.

fn main() {
    println!("cargo::rustc-check-cfg=cfg(text_parley)");
    println!("cargo::rustc-cfg=text_parley");
}
