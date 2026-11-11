# Lesson 9: three small ideas from the Mac build

For Narayan. Short on purpose.

## 1. `#[cfg(target_os = "macos")]`: an `if` that runs at build time
In React you might write `if (process.platform === "darwin") { ... }` and the code still ships to every platform. In Rust, `#[cfg(target_os = "macos")]` above a function means: **on any other OS this function does not exist**; the compiler never sees it.

Why it mattered: the agent wrote two small helper functions in `build.rs` that only the macOS branch calls. On Windows nobody called them, and our CI treats warnings as errors (`-D warnings`), so Windows failed with "function never used" (`dead_code`). Fix: put the same `#[cfg]` above the helpers. Rule of thumb: code that is only used behind a `cfg` gets the same `cfg`.

## 2. A hash is the integrity line of `package-lock.json`
Our Mac libvips is a zip in a private repo. CI downloads it and compares its SHA-256 with the value written in the repo: `6bb24d51...dba64`. If someone swapped the file, or a rebuild produced different bytes, the hash differs and the job stops. Same idea as the `integrity` field in `package-lock.json`, and the reason the release asset (not the recipe) is the real artifact.

Try it (Terminal, changes nothing in the project):
```
echo hello > /tmp/a.txt; shasum -a 256 /tmp/a.txt
echo hello! > /tmp/a.txt; shasum -a 256 /tmp/a.txt
```
One character changes the whole hash.

## 3. A guard you never saw say "no" is not a guard
Lesson 7 and 8 said it for scripts; this is the same for CI. The agent pushed a throwaway branch that asks for a release that does not exist (`macos-arm64-vips-nonexistent-negtest`). Result: the macOS job went red at "Install macOS prerequisites" with a clear message, while Windows stayed green. Only now do we know the "fail closed" promise is real. It is like testing that a route guard really returns 401 for a request without a token, instead of reading the middleware and believing it.

## 4. Where this leaves the Mac build
Both OSes now run the same proof in CI: the packaged app, with a clean environment, decodes the test HEIC and says `heic_decode_verified: true`. What CI cannot know is how **your** iPhone files look. That is your next check (NEXT_STEPS).
