---
name: fileop-safety-review
description: Use after writing or changing any Rust code that moves, copies, renames, trashes, restores or otherwise touches user files, and before reporting. Self-review against the spec section 10 safety invariants.
---

# File operation safety review

Answer each question with a file path and line, or "not applicable":

1. Is every mutation reached only through `OperationManager`? Search for `remove_file`, `remove_dir`, `fs::rename`, `fs::copy` outside the operations module.
2. Is there a stored plan, a confirmation bound to the plan hash, and revalidation of each source fingerprint at execution?
3. Is anything overwritten without an explicit, planned conflict policy?
4. Cross-volume: is the source removed only after the destination is fully written, fsynced, size-checked and BLAKE3-verified?
5. Is deletion OS Trash only, with refusal (not permanent deletion) when Trash is unavailable?
6. Is each item's state persisted before and after the step, and does startup recovery handle every state?
7. Are destinations canonicalised and contained inside a registered root (no `..`, no symlink escape, not inside the source)?
8. Are Windows cases handled: long paths, reserved names, locked files, case-insensitive collisions? macOS: NFC/NFD?
9. Is there an injected-failure test for each boundary (after stage, after copy, after verify, before source removal)?
10. Could any path make the last copy of something disappear? If unsure, stop and report.
