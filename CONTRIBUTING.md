# Contributing to Klypse

Thanks for helping improve Klypse.

## Before opening a change

- Open an issue first for large features or user-interface changes.
- Keep changes focused and avoid committing generated build artifacts, logs, or personal captures.
- Add or update tests when behavior changes.
- Keep user-facing text available in English and update the French translation catalog when needed.

## Local verification

Run the complete release gate from the repository root:

```bash
./scripts/dev-container.sh bash scripts/verify-release.sh
```

For a smaller Rust-only change, run formatting and the relevant package tests first:

```bash
cargo fmt --all -- --check
cargo test -p klypse-image --locked
```

Replace `klypse-image` with the crate affected by the change.

Packaging changes should also be checked with the corresponding Debian or Flatpak build and test scripts documented in the README.

## Tool-assisted contributions

Disclose substantial use of AI or automated code-generation tools in the pull-request description. Review and understand generated changes before submitting them, verify their licences and provenance, and never provide private captures, credentials, or unpublished vulnerability details to an external tool.

## Pull requests

Describe the user-visible effect, the environments tested, and any known limitation. Do not include screenshots or recordings containing private information.

By contributing, you agree that your contribution is licensed under GPL-3.0-or-later, matching the project license.
