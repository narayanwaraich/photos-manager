# Lesson 5: why builds rot, and what "pinning" really means (the cairo and pango failures)

**Where to look:** the failed run 42559034497 on GitHub (Actions tab, the red "Build & Test Windows x64 libvips" run), and `deps/win64-vips/` in the repo.

## 1. Two failures, one cause
Round 1 of TASK-0E failed twice in the build container: first because a library called *cairo* had a newer version than the patch expected, then because *pango* did not compile with the newer compiler. Nobody changed our code between the two. What changed is the **recipe's starting point**: the build begins with `FROM ghcr.io/libvips/build-win64-mxe:latest`. `latest` is a label that moves. In JavaScript terms it is `npm install somepackage@latest` without a lockfile: you get whatever is newest on the day you run it.

## 2. Three levels of "fixed version"
| Level | JavaScript | This project |
|---|---|---|
| a moving label | `"react": "latest"` | `...build-win64-mxe:latest` (what upstream uses, what broke) |
| a version number | `"react": "19.2.0"` | `rust-toolchain.toml` = `1.99.1`, `cairo_VERSION := 1.18.4` |
| a content hash | the integrity hash in `package-lock.json` | `@sha256:<digest>` after the image name, `sha256` of the vips zip in `ci.yml` |
The strongest pin is the hash: a version number can be re-published with different content, a hash cannot. That is why decision D14 says: pin the base image by digest, instead of fixing each drifted package one at a time.

## 3. Why a compiler said "no"
The pango error was `variable 'pango_font_face_parent_class' set but not used [-Werror]`. A **warning** is the compiler saying "this looks odd". `-Werror` turns every warning into an error. pango's authors compile with a strict list; a newer compiler (Clang 19) added one more check; the code behind a standard GLib macro trips it. Nothing is wrong with the program: the build is simply stricter than the code. A flag like `-Wno-error=...` silences it, but it fixes one symptom and leaves the cause (an image that moves).

## 4. Small CI ideas you saw this week
- `if: github.event_name == 'workflow_dispatch'` on a job: run it only when someone asks.
- `concurrency: cancel-in-progress`: a new push cancels the older run of the same branch.
- `paths-ignore`: pushes that only touch docs do not run CI at all. `[skip ci]` in a commit message does the same for one commit.
- **Artifact versus release**: an *artifact* is a temporary file of one run; a *release* is a permanent public download. In a public repository both can be downloaded by strangers, which is why a file containing an HEVC decoder is kept out of releases (D13).

## Try it (15 minutes)
1. `gh run view 42559034497 --log-failed | grep -n "error:"` and find the pango line. Which flag does it say turned the warning into an error?
2. Open `deps/win64-vips/UPSTREAM.txt`. Which of the three levels above does it use for the upstream repository (hint: it is a 40-character commit)?
3. In `.github/workflows/ci.yml` find one `concurrency:` block and one `paths-ignore:` list. Say in one sentence what each saves.
4. One sentence: "A moving `latest` label is risky in a build because ...".
Book: nothing for this one; it is about tooling, not Rust. Next Rust lesson: `enum` and `match` (loaders and errors).
