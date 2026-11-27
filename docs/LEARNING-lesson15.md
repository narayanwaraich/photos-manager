# Lesson 15: "I could not tell" is a third answer, not a "no"

**The idea.** A yes/no check has a hidden third outcome: the check itself did not finish (a sleeping disk, a network share that never answers, a thread that panicked). If the code squeezes that into "no", it acts on something it does not know. In TASK-1F the question is "is this folder there?". Answering "no" because the disk was slow would mark the folder Offline; answering "yes" because nothing said otherwise would let a scan run on the wrong place. So the check returns one of three values, and only a definite answer is allowed to change anything.

**In the project code** (branch `task/1F`, now `main` after the merge):

```bash
cd ~/Desktop/Projects/photos-manager
grep -n "enum AvailabilityOutcome" -A 8 src-tauri/src/library/availability.rs
grep -n "Unknown" src-tauri/src/library/availability.rs
```

The first command shows the three variants (`Available`, `NotAvailable`, `Unknown`). In `apply_availability_result` the rule is one `match`: `Active` plus `NotAvailable` becomes `Offline`; `Offline` plus `Available` becomes `Active`; everything else, including every `Unknown`, falls into `_ => None` and writes nothing.

**Try it yourself (2 minutes).** Save this as `/tmp/three.rs` and run it. Nothing in the project is touched.

```rust
#[derive(Debug, Clone, Copy)]
enum Check { Available, NotAvailable, Unknown }

fn next(status: &'static str, c: Check) -> &'static str {
    match (status, c) {
        ("active", Check::NotAvailable) => "offline",
        ("offline", Check::Available) => "active",
        _ => status,
    }
}

fn main() {
    for (s, c) in [("active", Check::Unknown), ("active", Check::NotAvailable),
                   ("offline", Check::Unknown), ("offline", Check::Available)] {
        println!("{s:8} + {c:?} -> {}", next(s, c));
    }
}
```

```bash
cd /tmp
rustc --edition 2021 three.rs
./three
```

Expected output:

```
active   + Unknown -> active
active   + NotAvailable -> offline
offline  + Unknown -> offline
offline  + Available -> active
```

Now change `Check::Unknown` in the first line of `main` to `Check::NotAvailable` and run it again: the first row flips to `offline`. That is the whole bug class: if "unknown" were folded into "not available", a slow disk would take every photo root offline.

**Why Rust helps here.** With a `bool` the compiler lets you forget the third case. With an `enum`, a `match` must name every variant or have a `_` arm, and the `_` arm is where you write "do nothing" on purpose. Delete the `_ => status,` line and the compiler refuses to build until you decide what the other cases do.

**What to ignore for now.** The `&'static str` in the snippet (a string that lives for the whole program; the project uses an enum for the status instead) and the `{s:8}` padding.

**The same idea, one level up.** Lesson 14 was "an empty folder is a complete walk": absence of evidence is not evidence of absence. Lesson 15 is its twin: absence of an answer is not the answer "no".
