use semver::Version;
use std::fs;
use toml_edit::{DocumentMut, Item};

// The set of the "anyrender" packages that are versioned together
const ANYRENDER_PACKAGES: &[&str] = &[
    "anyrender",
    "anyrender_vello",
    "anyrender_vello_cpu",
    "anyrender_svg",
];

// The set of the "blitz" packages that are versioned together
const BLITZ_PACKAGES: &[&str] = &[
    "blitz",
    "blitz-dom",
    "blitz-html",
    "blitz-net",
    "blitz-paint",
    "blitz-shell",
    "blitz-traits",
    "stylo_taffy",
];

macro_rules! bail {
    ($err:expr) => {{
        eprintln!();
        eprintln!("{}", $err);
        std::process::exit(1);
    }};
}

fn with_cargo_toml(package_path: &str, cb: impl FnOnce(&mut DocumentMut)) {
    let cargo_toml_path = format!("{package_path}/Cargo.toml");
    let cargo_toml_str = fs::read_to_string(&cargo_toml_path).unwrap();
    let mut cargo_toml_doc = cargo_toml_str
        .parse::<DocumentMut>()
        .expect("invalid Cargo.toml");

    cb(&mut cargo_toml_doc);

    fs::write(&cargo_toml_path, cargo_toml_doc.to_string()).unwrap();
}

fn set_package_version(dep_name: &str, version: &str) {
    let package_path = format!("./packages/{dep_name}");
    with_cargo_toml(&package_path, move |cargo_toml| {
        cargo_toml["package"]["version"] = Item::from(version);
    });
}

fn set_workspace_version(version: &str) {
    with_cargo_toml(".", move |cargo_toml| {
        cargo_toml["workspace"]["package"]["version"] = Item::from(version);
    });
}

fn set_workspace_dep_version(dep_name: &str, version: &str) {
    with_cargo_toml(".", move |cargo_toml| {
        cargo_toml["workspace"]["dependencies"][dep_name]["version"] = Item::from(version);
    });
}

fn main() {
    // --- Parse CLI args

    let mut args = std::env::args().skip(1);

    // Parse "target" CLI arg
    let target = args.next();
    let target = match target.as_deref() {
        Some(target @ ("blitz" | "anyrender")) => target,
        Some(target) => {
            println!("{target}");
            bail!("Invalid target. Must be 'blitz' or 'anyrender'")
        }
        _ => bail!("Missing target. Must be 'blitz' or 'anyrender'"),
    };

    // Parse "version" CLI arg
    let version = args.next();
    let version = match version {
        Some(version) => {
            if Version::parse(&version).is_ok() {
                version
            } else {
                bail!("Invalid version. Must be valid cargo version.")
            }
        }
        _ => bail!("Missing version Must be valid cargo version."),
    };

    // --- Bump versions

    if target == "anyrender" {
        for package in ANYRENDER_PACKAGES {
            set_package_version(package, &version);
            set_workspace_dep_version(package, &version);
        }
        println!("Bumped anyrender versions")
    }

    if target == "blitz" {
        set_workspace_version(&version);
        for package in BLITZ_PACKAGES {
            set_workspace_dep_version(package, &version);
        }

        println!("Bumped blitz versions")
    }
}
