#[test]
fn version_script_allows_cargo_to_fetch_missing_crates() {
    let script = include_str!("../scripts/version.sh");

    assert!(
        !script.contains("cargo check --offline"),
        "version script should not require a pre-populated Cargo cache"
    );
    assert!(
        script.contains("cargo check"),
        "version script should still validate the updated manifest"
    );
}
