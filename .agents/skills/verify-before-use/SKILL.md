---
name: verify-before-use
description: Use before adding or using any crate, npm package, Tauri plugin, native library, CLI flag, or cloud API/model ID. Verifies it exists and checks the real current version and API instead of recalling from training data.
---

# Verify before use

Your training knowledge ends around March 2026. Treat every version, API shape, flag and model ID as unverified until checked.

1. Rust crate: run `cargo info <name>` (version, license, repository). Check the repo for recent commits and open issues about the target OS. Read docs.rs for the exact API you will call.
2. npm package: run `npm view <name> version license repository.url`. Confirm it is the package you mean (not a look-alike name).
3. Tauri plugin or API: read the official Tauri 2 docs page for it; confirm the permission identifiers it needs. Do not guess permission names.
4. Native library or CLI: run `<tool> --version` and `<tool> --help`; quote only flags you saw.
5. Cloud model IDs and request shapes: read the provider's official docs page. If you cannot verify, write "unverified" and make it a setting, never a constant.
6. Record in your report: name, exact version, source URL, license, date checked. If a check fails, stop and say so; do not substitute from memory.
