# Photos Manager: Path Model

This document explains how Photos Manager represents, stores, normalizes, and compares file locations across macOS and Windows (spec §5.1, §6.4, decisions D26 & D27).

---

## 1. Native Path BLOB Encoding

Filesystems represent paths as raw OS-native bytes rather than guaranteed UTF-8 strings.
To ensure completely lossless path round-tripping for any filename on the user's disk:

- **Unix / macOS:** Paths are stored as the exact sequence of bytes returned by the OS (`std::os::unix::ffi::OsStrExt::as_bytes()`).
- **Windows:** Paths are stored as 16-bit little-endian code units (`UTF-16LE`, 2 bytes per code unit, via `std::os::windows::ffi::OsStrExt::encode_wide()`). An odd byte length is rejected as an invalid native path, preventing silent truncation.

### The "Skip & Report" Unicode Policy
While the low-level `NativePath` converter is 100% lossless for any OS byte sequence, the application's catalogue **requires valid Unicode**.
- On Unix: Any path component containing invalid UTF-8 bytes returns `PathError::NotUnicode` (stable code `path_not_unicode`).
- On Windows: Any path component containing an unpaired surrogate returns `PathError::NotUnicode`.

The scanner skips unrepresentable paths and reports them as warnings to the user; lossy replacements (such as `\u{FFFD}`) are **never** stored in the database.

---

## 2. Root-Relative Path (`RelPath`)

Absolute paths are never stored for individual files or folders. Instead, they are stored relative to their registered `library_roots` entry. This allows an external drive to be remounted at a different mount point or drive letter by updating a single row in `library_roots`.

`RelPath` enforces strict invariants without touching the disk or canonicalising:
1. **Normal components only:** It rejects parent traversals (`..`), current directory components (`.`), root prefixes (e.g. `/` or `C:`), and empty components.
2. **Safety checks:** Components containing NUL characters (`\0`) or path separators (`/` or `\`) within a single component name are rejected.
3. **Display string:** `rel.display()` produces valid UTF-8, always formatted with forward slashes (`/`) regardless of operating system.
4. **Verbatim prefix handling (Windows):** Verbatim roots (`\\?\C:\...`) and verbatim file paths work seamlessly. Mixing a verbatim root with a non-verbatim path returns `PathError::VerbatimMismatch`.

---

## 3. The Comparison Key (`path_key`)

To prevent phantom duplicates and handle differences in volume formats and operating systems, every file and folder has a derived `path_key` (stored as `BLOB`):

1. **Unicode NFC Normalization:** Every path component is normalized to Canonical Decomposition followed by Canonical Composition (Unicode NFC) using the `unicode-normalization` crate.
   *Example:* On macOS, filenames created via APFS/HFS+ often use decomposed NFD (`e` + `\u{0301}`), whereas files transferred from Windows or Linux use precomposed NFC (`é` / `\u{00E9}`). Both spellings produce identical `path_key` bytes.
2. **Case Sensitivity Policy:**
   - On **case-insensitive volumes** (`case_sensitive = 0`, typical for APFS default and Windows NTFS): Components are folded using `str::to_lowercase()` after NFC normalization. `A.JPG` and `a.jpg` yield the exact same key.
   - On **case-sensitive volumes** (`case_sensitive = 1`, e.g. case-sensitive APFS or ext4): Original casing is preserved after NFC normalization.
3. **Separator Independence:** Path components are joined strictly by forward slashes (`/`) and encoded as UTF-8 bytes.

### Limits of Case Folding
Photos Manager uses standard Unicode case folding (`str::to_lowercase`). Two subtle linguistic edge cases to be aware of:
- **Greek Final Sigma:** Capital Greek sigma $\Sigma$ (U+03A3) lowercases to $\varsigma$ (U+03C2) at word ends and $\sigma$ (U+03C3) elsewhere.
- **Turkish Dotted/Dotless I:** In standard Unicode, capital `I` lowercases to dotless `i` (U+0069), whereas in Turkish locale rules it lowercases to dotless `ı` (U+0131).

A false collision in `path_key` is reported as a warning, never silently merged.

---

## 4. Key Collisions (`path_key_collision`)

SQLite enforces a `UNIQUE(root_id, path_key)` constraint on both `folders` and `files`.
If a newly scanned file or folder produces an identical `path_key` to an existing record in the same root, the database repository returns `CatalogueError::PathKeyCollision` (code `path_key_collision`). This is treated as a non-fatal warning by the scanner.

---

## 5. Uniform Hierarchy: The Root-Folder Row (D27)

To ensure the folder tree is completely uniform and avoid ambiguity around whether a `NULL` folder ID means "in the root" or "orphaned":
- Every registered `library_roots` entry has exactly one root-folder row in `folders`.
- The root-folder row has: `parent_id = NULL`, `name = ""`, empty relative path, and empty `path_key`.
- Top-level folders inside the root have the root-folder row as their `parent_id`.
- **Every physical file has a non-NULL `folder_id`** referencing its containing folder, enforcing consistent tree navigation.

---

## 6. Catalogue Platform Guard (D26)

Because macOS and Windows store native path BLOBs in different encodings (raw bytes vs UTF-16LE), opening a catalogue database on a different OS architecture would corrupt path reconstruction.

When a database is first created, `init_pool` writes a stamp into `app_settings`:
`key = "native_path_encoding"`, `value_json = "\"unix-bytes\""` (or `"\"windows-utf16le\""`).
When an existing database is reopened:
- If the stamp matches the running OS, connection proceeds normally.
- If the stamp differs (e.g. a database copied from macOS to Windows), `init_pool` returns `CatalogueError::PlatformMismatch` (stable code `catalogue_platform_mismatch`), preventing database corruption.
