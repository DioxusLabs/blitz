//! Picks the text backend, as the `text_parley` or `text_winkin` cfg, which the
//! code reads in place of the features.

fn main() {
    println!("cargo::rustc-check-cfg=cfg(text_parley)");
    println!("cargo::rustc-check-cfg=cfg(text_winkin)");
    if std::env::var_os("CARGO_FEATURE_WINKIN").is_some() {
        println!("cargo::rustc-cfg=text_winkin");
    } else if std::env::var_os("CARGO_FEATURE_PARLEY").is_some() {
        println!("cargo::rustc-cfg=text_parley");
    }
}
