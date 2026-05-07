use std::process::Command;

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

#[test]
fn homebrew_formula_script_updates_urls_and_checksums_from_release_sums() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let formula_dir = temp.path().join("Formula");
    std::fs::create_dir(&formula_dir).expect("create Formula dir");
    std::fs::write(
        formula_dir.join("git-ws.rb"),
        include_str!("../Formula/git-ws.rb"),
    )
    .expect("write formula");
    let sums_path = temp.path().join("SHA256SUMS");
    std::fs::write(
        &sums_path,
        "\
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  git-ws-v1.2.3-aarch64-apple-darwin.tar.gz
bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb  git-ws-v1.2.3-x86_64-apple-darwin.tar.gz
cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc  git-ws-v1.2.3-x86_64-unknown-linux-gnu.tar.gz
",
    )
    .expect("write sums");

    let output = Command::new("sh")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/scripts/update-homebrew-formula.sh"
        ))
        .arg("v1.2.3")
        .arg(&sums_path)
        .current_dir(temp.path())
        .output()
        .expect("run update script");

    assert!(
        output.status.success(),
        "script failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let updated = std::fs::read_to_string(formula_dir.join("git-ws.rb")).expect("read formula");
    assert!(updated.contains("releases/download/v1.2.3/git-ws-v1.2.3-aarch64-apple-darwin.tar.gz"));
    assert!(
        updated.contains(
            "sha256 \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\""
        )
    );
    assert!(updated.contains("releases/download/v1.2.3/git-ws-v1.2.3-x86_64-apple-darwin.tar.gz"));
    assert!(
        updated.contains(
            "sha256 \"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\""
        )
    );
    assert!(
        updated.contains("releases/download/v1.2.3/git-ws-v1.2.3-x86_64-unknown-linux-gnu.tar.gz")
    );
    assert!(
        updated.contains(
            "sha256 \"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc\""
        )
    );
}

#[test]
fn release_workflow_updates_homebrew_formula_after_publishing_release() {
    let workflow = include_str!("../.github/workflows/release.yml");

    assert!(workflow.contains("update-homebrew-formula:"));
    assert!(workflow.contains("- publish"));
    assert!(
        workflow
            .contains(r#"scripts/update-homebrew-formula.sh "$RELEASE_VERSION" dist/SHA256SUMS"#)
    );
    assert!(workflow.contains(r#"git push origin "HEAD:${DEFAULT_BRANCH}""#));
}
