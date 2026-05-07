use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

#[test]
fn version_script_updates_manifest_and_runs_plain_cargo_check() {
    let temp = tempfile::tempdir().expect("create tempdir");
    std::fs::create_dir(temp.path().join("src")).expect("create src dir");
    std::fs::write(
        temp.path().join("Cargo.toml"),
        r#"
[package]
name = "version-script-test"
version = "0.1.0"
edition = "2024"
"#,
    )
    .expect("write Cargo.toml");
    std::fs::write(temp.path().join("src/lib.rs"), "").expect("write lib.rs");
    let cargo_log = temp.path().join("cargo-args.log");
    let fake_bin = temp.path().join("bin");
    std::fs::create_dir(&fake_bin).expect("create fake bin");
    let fake_cargo = fake_bin.join("cargo");
    std::fs::write(
        &fake_cargo,
        r#"#!/bin/sh
printf '%s\n' "$*" > "$CARGO_ARGS_LOG"
if [ "$1" = "check" ] && [ "$#" -eq 1 ]; then
  exit 0
fi
printf 'unexpected cargo args: %s\n' "$*" >&2
exit 1
"#,
    )
    .expect("write fake cargo");
    let mut permissions = std::fs::metadata(&fake_cargo)
        .expect("fake cargo metadata")
        .permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&fake_cargo, permissions).expect("chmod fake cargo");

    let output = Command::new("sh")
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/version.sh"))
        .arg("1.2.3")
        .current_dir(temp.path())
        .env("CARGO_ARGS_LOG", &cargo_log)
        .env("PATH", prepend_path(&fake_bin))
        .output()
        .expect("run version script");

    assert!(
        output.status.success(),
        "script failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest = std::fs::read_to_string(temp.path().join("Cargo.toml")).expect("read manifest");
    assert!(manifest.contains(r#"version = "1.2.3""#));
    assert_eq!(
        std::fs::read_to_string(cargo_log).expect("read cargo args"),
        "check\n"
    );
}

#[test]
fn homebrew_formula_script_updates_urls_and_checksums_from_release_sums() {
    let temp = tempfile::tempdir().expect("create tempdir");
    let formula_dir = temp.path().join("Formula");
    std::fs::create_dir(&formula_dir).expect("create Formula dir");
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
    for (target, sha_byte) in [
        ("aarch64-apple-darwin", 'a'),
        ("x86_64-apple-darwin", 'b'),
        ("x86_64-unknown-linux-gnu", 'c'),
    ] {
        let url = format!("releases/download/v1.2.3/git-ws-v1.2.3-{target}.tar.gz");
        let sha_line = format!("sha256 \"{}\"", sha_byte.to_string().repeat(64));
        assert!(updated.contains(&url), "missing url for {target}");
        assert!(updated.contains(&sha_line), "missing sha256 for {target}");
    }
}

fn prepend_path(path: &Path) -> String {
    format!(
        "{}:{}",
        path.display(),
        std::env::var_os("PATH")
            .unwrap_or_default()
            .to_string_lossy()
    )
}
