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

(Next lessons planned: Lesson 2 Rust: structs, enums and traits (the `ImageProcessor` trait); Lesson 2 Git: reading a diff and branching for a fix; Lesson 3 Rust: ownership and borrowing in depth; Lesson 3 Git: GitHub Actions and pull requests.)
