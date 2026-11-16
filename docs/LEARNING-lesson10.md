# Lesson 10: four ideas behind TASK-1A

For Narayan. Short on purpose. No code to run in the project; the two experiments need only a browser console.

## 1. A file name is not always text
In JavaScript a path is a string, and strings are text. On disk it is messier: on macOS and Linux a file name is a sequence of **bytes** (usually UTF-8, but nothing forces it), on Windows it is a sequence of **16-bit units** (usually UTF-16, but a stray half of a pair is allowed). A few real files have names that are not valid text. Node would hand you a string with `�` where the broken bytes were, and you could never open that file again by that name.

Rust has a separate type for this (`OsString`, `Path`), and the catalogue stores the **raw bytes** (a `BLOB` column) next to a readable copy for the screen. Lossless storage, readable display: two columns for two jobs. Our rule (D26): if a name is not valid text we **skip it and report it** rather than store a damaged copy.

## 2. Two names that look identical can be different
Open the browser console and try:
```js
"é" === "é"                       // false: one character vs "e" + a combining accent
"é".length                              // 1
"é".length                        // 2
"é" === "é".normalize("NFC")      // true
```
macOS can hand a file name back in one form and Windows in the other, so the same photo could get two catalogue entries. The fix is a **key** for each path: every part is converted to the same form (NFC) and, on drives that ignore capital letters, lower-cased. Two files with the same key in the same folder cannot both exist, so a clash is reported instead of silently merged (D26).

## 3. Typed ids: TypeScript "branded types", but enforced
You may know this trick: `type FileId = string & { __brand: "FileId" }`, so a function that wants a `FileId` refuses an `AssetId`. In Rust it is built in and costs nothing at run time:
```rust
struct FileId(String);
struct AssetId(String);
```
`fn load(id: FileId)` will not compile if you pass an `AssetId`. TASK-1A uses this for the five kinds of id (volume, root, folder, file, asset), because mixing them up is exactly the bug that deletes the wrong thing later.

## 4. A migration that has run is never edited
Same rule as with Prisma or Knex: once a migration has been applied somewhere, you add a **new** file instead of changing the old one. SQLx goes further and remembers a checksum of every applied migration; if the file changes, the app refuses to start. That is why TASK-1A adds one new migration file for the Phase 1 tables and leaves the Phase 0 one alone, and why one of its tests builds a database with only the old migration, adds a row, runs the new one, and checks that the row survived.

**One more habit from lesson 9:** the task asks the agent to show the "remove a folder" test failing against my old, buggy table definition before showing it pass. A test you have never seen fail may not be testing anything.
