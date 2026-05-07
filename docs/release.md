# Release Procedure

This document is for maintainers publishing a release. For normal build and
test commands, see [development.md](development.md).

## Prerequisites

- The working tree is clean.
- `gh auth status` succeeds for the target repository.
- CI is green on the commit to release.
- You have permission to push tags and create GitHub Releases.

## Normal Release

1. Update the package version.

   ```sh
   scripts/version.sh X.Y.Z
   ```

2. Review the version diff.

   ```sh
   git diff -- Cargo.toml Cargo.lock
   ```

3. Commit and tag the release.

   ```sh
   git add Cargo.toml Cargo.lock
   git commit -m "chore: release vX.Y.Z"
   git tag vX.Y.Z
   git push origin HEAD --tags
   ```

4. Open GitHub Actions and confirm the `Release` workflow succeeds.

5. Confirm the GitHub Release contains:

   - `git-ws-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz`
   - `git-ws-vX.Y.Z-aarch64-apple-darwin.tar.gz`
   - `git-ws-vX.Y.Z-x86_64-apple-darwin.tar.gz`
   - `SHA256SUMS`

## Manual Dispatch

Use `workflow_dispatch` only after the matching tag already exists.

1. Open the `Release` workflow in GitHub Actions.
2. Choose `Run workflow`.
3. Enter `vX.Y.Z` in the `version` input.
4. Confirm the workflow validates `Cargo.toml` version before building.

## Failure Recovery

- If version validation fails, fix `Cargo.toml` or create the correct tag. Do not edit the workflow run.
- If artifact build fails, fix the code and create a new release commit and tag.
- If only release creation fails after all artifacts are built, rerun the failed workflow job or dispatch the same existing tag manually.
- If a bad tag was pushed, delete the local and remote tag before recreating it:

  ```sh
  git tag -d vX.Y.Z
  git push origin :refs/tags/vX.Y.Z
  ```
