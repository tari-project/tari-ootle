# Checklists

## Prepare release checklist

- [ ] Decide on the next version number
    - If breaking changes:
    - [ ] Bump major version
    - If no breaking changes:
    - [ ] Bump minor version
- [ ] Update the workspace version in `Cargo.toml`
- [ ] Create a PR for the version bump
    - Use the title `chore: release vX.Y.Z`

## Release checklist

- [ ] Create the tag for the release
    - Use `git tag -s vX.Y.Z -m "vX.Y.Z"`. This ensures the tag is signed.
- [ ] Push the tag to the remote repository
    - Use `git push remote vX.Y.Z` where `remote` is the name of the remote repository (e.g. `origin`/`upstream_mut`).
- [ ] Wait for binaries to be built by the CI/CD pipeline
- [ ] Create a release on GitHub
    - Use the tag created in the previous step
    - Add release notes, as needed