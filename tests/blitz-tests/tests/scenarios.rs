//! Runs every JSON scenario fixture in `tests/blitz-tests/scenarios/`.

use blitz_test_harness::run_scenario_file;

#[test]
fn run_all_scenarios() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scenarios");
    let mut count = 0;
    for entry in std::fs::read_dir(&dir).expect("scenarios dir exists") {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "json") {
            println!("running scenario {}", path.display());
            run_scenario_file(&path);
            count += 1;
        }
    }
    assert!(
        count >= 2,
        "expected scenario fixtures in {}",
        dir.display()
    );
}
