# Learning Rust and Git with this project

Written for a React / full-stack JavaScript developer. Each lesson uses real code from this repo. Lessons are added here after each review; the newest one is also copied into `docs/NEXT_STEPS.md`. Nothing here is required to keep the project moving. It is there so that by Phase 2 you can read every line the agent writes.

Free reference to keep open: *The Rust Programming Language* ("the Book"), https://doc.rust-lang.org/book/ . Do chapters 3, 4, 6 and 9 in order over the next weeks; they match lessons 1–3 below.

---

## Lesson 1 (Rust): Option, Result, `?`, async and ownership: the ExifTool timeout bug

**Where in the code:** `src-tauri/src/metadata/mod.rs`, function `extract_metadata`. Run `cargo test metadata` to see its tests.

### 1. No `undefined`, no `throw`
| JavaScript | Rust | Meaning |
|---|---|---|
| `string \| undefined` | `Option<String>` | A value or nothing: `Some("x")` or `None` |
| a function that may `throw` | `Result<T, E>` | Either `Ok(value)` or `Err(error)`; the error is part of the return type |
| `try { x = await f() } catch (e) { throw e }` | `let x = f().await?;` | The `?` means: "if this is an error, return it from my function now" |
| `switch` | `match` | The compiler forces you to handle every case |

In `extract_metadata` you will see `Result<NormalizedMetadata, MetadataError>`. `MetadataError` is a list of everything that can go wrong (`Timeout`, `FileNotFound`, ...). A caller cannot forget an error case: the compiler stops them.

### 2. `async` and `.await` look familiar but differ in one way
`async fn` and `.await` work like JavaScript's. The difference: a JavaScript `Promise` starts running the moment you create it. A Rust future does **nothing** until something awaits it. Rust async needs a runtime to drive it; this project uses `tokio`.

### 3. The line with two question marks
```rust
let (status, output_bytes) = tokio::time::timeout(timeout, read_future)
    .await
    .map_err(|_| MetadataError::Timeout(timeout))??;
```
JavaScript equivalent:
```js
const [status, bytes] = await Promise.race([work, sleep(timeout).then(() => { throw new Timeout() })]);
```
`timeout(...).await` returns a `Result` whose success value is itself the `Result` of the work. The first `?` unwraps the timeout layer, the second unwraps the work's own result. Two layers, two `?`.

### 4. Ownership: why the bug existed
Rust has no garbage collector. Every value has exactly one **owner**. When the owner goes out of scope the value is **dropped** (its cleanup code runs automatically, a bit like a `finally` that you never write). You can lend a value with `&` (a borrow) without giving it away.

Our bug: the ExifTool process was represented by a `Child` value. On timeout the function returned and the `Child` was dropped, but tokio's default is that dropping a `Child` does **not** kill the process. So every timeout left an ExifTool running in the background. In JavaScript it is like starting a `child_process.spawn` and racing it against a timer but never calling `.kill()`.

The fix was `kill_on_drop(true)` (tell tokio to kill the process when the handle is dropped) plus an explicit `child.kill().await`. The test `test_child_killed_after_timeout` starts a script that records its process id, forces a timeout, then checks that the id no longer exists.

### Try it (15 minutes)
1. Open `src-tauri/src/metadata/mod.rs`. Find `kill_on_drop`. Read the 10 lines around it.
2. Find the `enum MetadataError`. Count its variants.
3. Run `cargo test metadata -- --nocapture` from the `src-tauri` folder (or `cargo test --manifest-path src-tauri/Cargo.toml metadata` from the repo root) and read the test names.
4. Write one sentence in your own words: "Dropping a `Child` does not kill the process because ...". Keep it in a notes file; you will use this in an interview someday.

Book chapters that match: 3 (common concepts), 4 (ownership), 9 (error handling).

---

## Lesson 1 (Git): what you just did

**Mental model**
- A **commit** is a snapshot of the whole project plus a message. Commits point to their parent, forming a chain.
- A **branch** is just a movable label pointing at a commit (`main`, `task/0A`). Creating a branch costs nothing.
- **HEAD** is "where you are now" (the commit/branch you have checked out).
- The **working tree** is the files on disk. The **staging area** is the list of changes you have chosen to include in the next commit (`git add` puts changes there).
- A **remote** (`origin`) is a copy of the repository on GitHub. `push` sends commits there; nothing goes to GitHub until you push.

**What you did, in plain words**
1. Made a prep commit on `main` (the shared `.gitignore`).
2. `git merge main` while on `task/0A`: brought the new `main` into the task branch once, so both agree on `.gitignore`. A **conflict** appears when two branches changed the same lines (here, both created `.gitignore`). You resolved it by taking `main`'s version (`--theirs`, because when you merge main *into* a branch, main is "theirs").
3. Now you will **squash-merge**: `git merge --squash task/0A` takes all changes from the branch and stages them as one change on `main`; you commit once. Result: `main` has one clean commit per task instead of every messy step. The branch itself stays untouched.
4. A **tag** (`accepted/0A`) is a permanent label on a commit: a bookmark for "this is what was accepted".

**Commands worth memorising**
| Command | What it tells you or does |
|---|---|
| `git status --short` | Which files changed (and whether anything is uncommitted) |
| `git log --oneline --graph --all` | A picture of all branches. Press `q` to leave the pager |
| `git diff` / `git diff --staged` | Changes not yet staged / already staged |
| `git show --stat HEAD` | What the latest commit changed |
| `git switch <branch>` or `git checkout <branch>` | Move to another branch (both work) |
| `git reflog` | A history of where HEAD has been. **Your safety net**: almost any mistake can be undone by finding the old commit here and `git checkout`-ing it onto a new branch |

**Rules that keep you safe**
- Run `git status --short` before any switch or merge. Empty output means nothing is at risk.
- Never run `git reset --hard`, `git push --force` or `git clean -fd` unless Claude tells you to and says why.
- If a command prints something you do not understand, stop and paste the whole output to Claude. Nothing is lost by pausing.

### Try it (10 minutes)
Run `git log --oneline --graph --all` in the repo and find: `main`, `task/0A`, `task/0B`, the merge commits (they have two parents, shown as lines joining), and later your new `accepted/0A` tag.

---

## Lesson 2 (Rust): traits are interfaces, and "stable" is not a pin

### 1. A trait is a TypeScript `interface`
| TypeScript | Rust |
|---|---|
| `interface ImageProcessor { thumbnail(...): Result }` | `trait ImageProcessor { fn thumbnail(...) -> Result<...>; }` |
| `class VipsProcessor implements ImageProcessor { ... }` | `impl ImageProcessor for VipsImageProcessor { ... }` |
| the class body and the `implements` are together | the struct is declared in one place and each `impl` block is separate |

Why the spec wants it (spec §3.3): product code talks to the trait, not to libvips. If the libvips binding ever has to be swapped (the fallback ladder), only the file with the `impl` changes. That is also why 0A built exactly one trait and one implementation.

**Try it (15 minutes).** I am describing this from the reports, so open the files and check: `src-tauri/src/image_pipeline/mod.rs` should hold the trait (about 40 lines) and `src-tauri/src/image_pipeline/vips.rs` the implementation.
1. What are the trait's method names, and what `Result` error type do they return?
2. Find the line that says `impl ... for ...`. Which type is on the right of `for`?
3. Find one test in `vips.rs` that calls the trait method. What fixture file does it use?
4. One sentence: "The benefit of the trait is ...".
Book: chapter 10 (generics and traits).

### 2. `rust-toolchain.toml`: `"stable"` versus a pinned version
Review of 0C found `channel = "stable"` in `rust-toolchain.toml`. In the JavaScript world that is like `"node": "latest"`: every machine installs whatever "stable" means on the day it runs, so your Mac, the Windows laptop and CI can compile with different compilers. A **pin** is the exact version (`"1.99.1"`, like `"node": "20.11.1"` in `.nvmrc`). Rule of thumb in this project: *lockfiles pin libraries, `rust-toolchain.toml` pins the compiler*. Run `rustc --version` to see which one you have today.

---

## Lesson 2 (Git): reading a diff, and fixing on the same branch

**Why 0C is not merged yet.** A review says FIX; the fixes go on the **same branch** (`task/0C`) as **new commits**. Nothing is rewritten. When it is accepted you squash-merge once, as before.

**Commands (try them on `task/0C`, 10 minutes):**
| Command | Meaning |
|---|---|
| `git diff --stat main...task/0C` | Three dots: "what the branch changed since it split from `main`". The summary the reviewer asks for |
| `git log --oneline main..task/0C` | Two dots: the commits that are on the branch but not on `main` |
| `git show --stat HEAD` | What the latest commit touched |
| `git rev-parse --short HEAD` | The short hash of where you are. Use this in the report header |

**Why the report header hash mattered.** A commit hash is the unique name of a snapshot. The 0C report named `5f73fc3`, but your log shows the latest commit is `0a3e075`. A report that cites a hash that is not in the history cannot be matched to the code. That is why the review asks for `git rev-parse --short HEAD` instead of a typed hash.

**Try it:** run the first two commands. How many commits does `task/0C` have that `main` does not? (Your log shows 15 or so.) Which commit message tells you the first Windows fix?

---

## Lesson 3 (Rust): "initialise once" with `OnceLock`, `unsafe` for C, and why debug builds can lie

**Where in the code:** `src-tauri/src/image_pipeline/vips.rs`, lines 8–36 (`get_vips_app`). Run `cargo test vips -- --nocapture` to see it work.

### 1. A global that is created once, safely
```rust
static VIPS_APP: OnceLock<VipsApp> = OnceLock::new();

fn get_vips_app() -> &'static VipsApp {
    VIPS_APP.get_or_init(|| { /* start libvips, block loaders */ app })
}
```
JavaScript equivalent: a module-level `let app; function getApp() { return app ??= init(); }`. The Rust version adds two guarantees JavaScript does not give you: it is safe when many threads call it at the same moment (only one runs the setup closure, the others wait), and the closure runs **exactly once** per process. Before 0C, every thumbnail request created its own libvips instance; this is the fix.
- `static` means "lives for the whole program".
- `&'static VipsApp` is a **borrow** that is valid for the whole program. The function hands out a reference; nobody takes ownership, so nobody can drop (destroy) libvips while it is in use.

### 2. `.expect("...")`: fine at start-up, risky per request
`.expect("msg")` means "give me the value, or crash the program with this message". Reviewers allow it when failing to start is the only sensible outcome (libvips cannot initialise at all), and dislike it inside request handlers, where one bad file should become an error, not a crash. That is why the review asks to move initialisation to app start in Phase 1 and return a `Result`.

### 3. `unsafe` is for talking to C
```rust
unsafe {
    bindings::vips_block_untrusted_set(1);
    bindings::vips_operation_block_set(c"svgload".as_ptr(), 1);
}
```
libvips is written in C. The Rust compiler cannot check C code, so calling it needs an `unsafe` block. It does not mean "dangerous code"; it means "the compiler cannot verify this, a human did". Think of calling a native Node addon: you trust it. `c"svgload"` is a C string literal (Rust adds the NUL byte C expects) and `.as_ptr()` turns it into the raw pointer the C function wants.

### 4. Environment variables are shared global state
`std::env::set_var("VIPS_BLOCK_UNTRUSTED", "1")` changes a setting that every thread in the process can read. Changing it while another thread reads it is a data race at the C level. Newer Rust editions mark `set_var` as `unsafe` for exactly this reason. It is harmless today (one thread at that moment) but belongs in a start-up function that runs before any other thread, which is the Phase 1 clean-up.

### 5. `cfg!(debug_assertions)`: the reason we test the package
`metadata/mod.rs` looks for a system ExifTool only when `cfg!(debug_assertions)` is true, which is the case in `pnpm tauri dev` and `cargo test`, not in a release build. So a test that passes on your Mac in dev mode can hide the fact that the release app cannot find its bundled tools. This is the whole reason 0C's smoke test must run the installed package with a stripped PATH.

### Try it (15 minutes)
1. Open `vips.rs` lines 8–36. How many places call `unsafe`? Why is none needed for `get_or_init`?
2. In `test_vips_blocks_svgload`, what does the test currently check about the error? (Round 2 asks for something stronger: what?)
3. Run `cargo test vips -- --nocapture` and find the line that prints which loaders are supported. Is `heifload` in it on your Mac?
4. One sentence: "`OnceLock` is better than creating libvips per request because ...".
Book chapters: 4 (ownership), 10 (lifetimes, the `'static` part), 16 (threads).

---

## Lesson 3 (Git): a commit cannot contain its own hash, and reading CI from the terminal

### 1. Why "match the report hash to HEAD" never works
A commit's hash is computed from its content. If a report file inside the commit contains that commit's own hash, changing the file changes the hash, which changes the file again. It never settles. So a report names the **last commit that changed code or CI** (plus the CI run id), and later commits only touch docs. You can prove that with:
```
git diff --stat <that-commit> HEAD
```
Two arguments, no dots: "what changed between these two commits". If it lists only `docs/` and `reports/`, the code is exactly what was tested.

### 2. Reading CI without clicking
| Command | What it gives |
|---|---|
| `gh run list --workflow ci.yml --branch task/0C --limit 3` | The latest runs with their ids and results |
| `gh run view <id>` | Each job and its duration |
| `gh run download <id> -D some-folder` | The files the run uploaded (artifacts), on your disk |
A CI workflow (`.github/workflows/ci.yml`) is a recipe: **jobs** run on a machine type, each job is a list of **steps**. Read the macOS job from top to bottom and you can see exactly what "tested" meant.

### 3. Why Syncthing and git do not mix
Git already is the tool that moves code between computers. Syncing a repository with Syncthing as well copies `.git` internals, `node_modules` and `target` (compiled for one operating system) to another one, and two tools then fight over the same files. Rule: **code goes through GitHub; photos and documents go through Syncthing.** If a folder must be shared with both, a `.stignore` file lists what to skip, but it is per computer, so it must exist on both before anything is deleted.

### Try it (10 minutes)
1. After the agent finishes round 2, run `git diff --stat <last code commit> HEAD`. Which kinds of files appear?
2. Run `gh run view` on the newest run id. How long did each job take?
3. Run `gh run download <id> -D ~/Desktop/ci-artifacts` and open the folder. Which artifact is the one you will test on the laptop?

---

(Next lessons planned: Lesson 4 Rust: `enum` and `match` in depth (the loader and error types), and `Cow`/strings (why a file path is not always text, which matters for Windows paths); Lesson 4 Git: pull requests and what "squash-merge" looks like on GitHub.)

