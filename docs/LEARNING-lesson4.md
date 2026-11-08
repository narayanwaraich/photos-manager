
---

## Lesson 4 (Rust and native code): what "linking" is, and why a missing library kills an app before it starts

**Where to look:** your Mac, any terminal. The same idea explains the 0C Windows failure (missing DLL, exit code -1073741515) and the 0D HEIC problem.

### 1. Static versus dynamic linking
A Rust program is compiled to one executable. Code it takes from other Rust crates is usually copied **into** that executable (static linking), like a JavaScript bundler inlining your dependencies. Code from C libraries such as libvips is different: the executable only records a promise, "at start-up, load `libvips.42.dylib` (macOS) or `libvips-42.dll` (Windows)" (dynamic linking). If the promise cannot be kept, the operating system refuses to start the program at all. It is like an `import` at the top of a file that fails before any of your code runs.

### 2. Reading those promises
| OS | Command | Shows |
|---|---|---|
| macOS | `otool -L <binary or .dylib>` | the libraries it will load, and from which path |
| Windows | `dumpbin /dependents <file>` | the same, for DLLs |
Try it (5 minutes): `otool -L src-tauri/target/debug/photos-manager` and count the lines that start with `/opt/homebrew`. Those are the Homebrew libraries the dev build depends on; the packaging script (`scripts/package-bundle.mjs`) copies them into the app and rewrites the paths. That is why the packaged app works without Homebrew.

### 3. The sneaky kind: libraries loaded later
`otool -L` only lists promises made at start-up. libvips can also load **modules** while running (the program calls `dlopen`, like `require()` inside a function). The macOS HEIF loader is such a module, so the packaging script, which only follows the start-up list, never copied it. That is why `heifload` was missing from the bundle although `libheif` itself was there.

### 4. A hard promise inside a library: the x265 problem
Homebrew's `libheif` lists `libx265` among its start-up promises. If the file `libx265` is not next to it, `dlopen` fails at once with "Library not loaded". The agent proved this with a tiny C program. In JavaScript terms: `libheif` has `import x265` at the top, not a lazy `import()`. To avoid shipping the GPL x265 you must build `libheif` yourself without it, not just leave the file out.

### 5. Try it (10 minutes)
1. Run `otool -L /opt/homebrew/lib/libheif.1.dylib`. Is `libx265` in the list?
2. Run `ls /opt/homebrew/lib/vips-modules-8.18/`. Which module files are there?
3. One sentence in your own words: "A library can be missing from a bundle even though `otool -L` of my app looks complete, because ...".
Book: chapter 19.1 (unsafe Rust and `extern` functions) gives the Rust side of calling C.

---

## Lesson 4 (Git and CI): a failed job is not always your fault, and how to tell

Your `main` branch run failed on Windows although you changed nothing but docs and one script. The log said `curl: (22) ... 504`. A **504** is a server-side error from the website you download from: the other side was down for a moment. The error message you actually saw first (`Cannot find path vips.zip`) was a **follow-on error**: the download failed, the script did not stop, and the next line failed because the file did not exist. It is like calling `fetch()`, ignoring `res.ok`, and then getting `Unexpected token` from `JSON.parse`. Rule: **read a failed log from the top, find the first red line, and ask whether it is an error in your code, in your setup, or on someone else's server.**

Commands (5 minutes, on any failed run you have):
| Command | What it does |
|---|---|
| `gh run view <id>` | which jobs failed |
| `gh run view <id> --log-failed` | only the log of the failed steps |
| `gh run rerun <id> --failed` | run again only the failed jobs (cheap: saves the green ones) |
| `gh run watch <id>` | live status until it finishes |
Why the fix is two-part: *retries* handle server hiccups; *stopping at the first error* (`$ErrorActionPreference = 'Stop'` in PowerShell, `set -e` in shell scripts) makes the next failure honest instead of misleading.

Try it: run `gh run view 42464971330 --log-failed` and find the first line that mentions `504`.
