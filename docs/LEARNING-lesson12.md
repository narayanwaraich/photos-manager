# Lesson 12: three habits from finishing TASK-1D

For Narayan. Short. Each point is real code from `main` (squash `605664c`); the commands are optional, 5 minutes each.

## 1. Asking how a worker ended is not the same as waiting for it
The scanner walks the folder on a background thread and sends batches to a writer. Before the last fix the writer did this after the batches stopped:

```rust
let _ = walk_handle.await;      // wait, and throw the answer away
```

In JavaScript this is `await worker.catch(() => {})`: you wait, but you never look at whether it threw. If the walking thread crashed halfway, the writer saw "no more batches" and treated it as "the whole folder has been seen", so every row it had not reached was marked missing. The fix reads the answer:

```rust
walk_handle.await.map_err(|_| ScanError::WalkFailed)?;
```

`.await` on a thread handle gives a `Result`: `Ok` if it finished, `Err` if it panicked. `map_err` turns that into our own error and `?` returns it early, before anything is marked missing. Rule: the end of a queue is not proof that the producer succeeded. Success is something you check.

Try it: `rg -n "walk_handle" src-tauri/src/scanner/mod.rs` and read the three lines around the hit.

## 2. A red CI is not always your code
Two of your runs went red with no code change: `45102343589` (the 1C2 merge) and `45181104275`. Both failed in `cargo deny`, the step that checks every library you use against a public list of known security problems (RUSTSEC advisories). The list changes every day. A library that was fine yesterday can be on it today, so the same commit goes from green to red. The fix was one command that raises one library to its patched version and changes only `Cargo.lock`:

```bash
cargo update -p url      # 2.5.8 -> 2.5.9
```

Habit from lesson 4, now with a name for this case: open the failed log, find the first red line, and ask "is this my code, my setup, or the outside world?" Here it was the outside world.

Try it: `gh run view 45102343589 --repo narayanwaraich/photos-manager --log-failed | grep -E "RUSTSEC|error"`.

## 3. A test switch inside real code is a trap
To prove the fix in point 1, the agent added a switch that makes the walker thread panic on purpose, and put it into the normal `ScanOptions` struct. It works, and the test is good, but now the production code contains a `panic!` that a caller can turn on. Nothing can today, because no button calls the scanner yet. TASK-1E adds the button, so its first job is to hide those switches from the real program with `#[cfg(test)]`, which means "compile this only when running tests". In JavaScript terms it is the difference between a helper inside `__tests__/` and a flag in `config.js`. Habit: a test hook is allowed, but it must not be able to ship.

Try it: `rg -n "panic_in_walker|panic_after_batch" src-tauri/src` and count the hits that are not inside `tests.rs`.
