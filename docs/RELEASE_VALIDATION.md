# v0.2.0 Mac Preview validation

2026-10-31. Platform: Apple Silicon ARM64, macOS 27. Intel Mac, Windows and older macOS versions have not been tested.

Release is a clean snapshot of the local workspace based on development commit ac75311b46f2ab230764641c3c98929973a5ad9d. Private reading data and development history are excluded.

- Rust/CLI: 99 tests passed (5 CLI, 23 unit, 9 Agent, 22 core, 9 journal, 25 relations, 6 runtime).
- Clippy: passed with -D warnings.
- Frontend: 15 tests passed; TypeScript and Vite production build passed.
- Python SDK: 3 unittest tests passed; expected fail-open warning exercised.
- Production reader-cli: release build passed.
- Production desktop: ARM64 app and DMG built successfully.
- No Developer ID signature or Apple notarization. Gatekeeper default trust and clean-machine installation are not certified.

Third-party licenses and MPL dependency source packages accompany the app and tools. Basic reading needs no Rust/npm/model/API key. Optional dependencies are described in README. Final desktop verification and protocol checks are documented in the GitHub Release.
