# Photos Manager: Path Model

How Photos Manager represents, stores, normalizes, and compares file locations across macOS and Windows (spec §5.1, §6.4, decisions D26 & D27).

---

## 1. Native Path BLOB Encoding

Filesystems store paths as raw OS bytes rather than guaranteed UTF-8 strings.
To guarantee lossless round-tripping for any filename on the user's disk:
- **macOS / Unix:** Stored as raw bytes returned by the OS (`OsStrExt::as_bytes()`).
- **Windows:** Stored as 16-bit little-endian code units (`UTF-16LE`, via `OsStrExt::encode_wide()`). An odd byte length is rejected as invalid, preventing truncation.

### The "Skip & Report" Policy
The catalogue requires valid Unicode. Path components containing invalid UTF-8 (Unix) or unpaired surrogates (Windows) return `PathError::NotUnicode`. Similarly, on macOS a filename containing a backslash or ending in a colon is legal on disk but is skipped and reported, ensuring cross-platform safety. Unrepresentable paths are never silently corrupted with replacement characters (`\u{FFFD}`).

---

## 2. Root-Relative Path (`RelPath`)

Absolute paths are never stored for files or folders. Paths are stored relative to a `library_roots` entry, allowing drives to be remounted or reassigned seamlessly.
`RelPath` enforces strict invariants without touching disk:
- Rejects parent traversals (`..`), current directory components (`.`), root prefixes (`/`, `C:`), and empty components.
- Components containing NUL (`\0`) or path separators (`/`, `\`) are rejected.
- `display()` produces valid UTF-8, formatted with forward slashes (`/`) on all operating systems.
- Verbatim paths on Windows (`\\?\C:\...`) work; mixing verbatim root with non-verbatim targets returns `PathError::VerbatimMismatch`.

---

## 3. The Comparison Key (`path_key`)

Every file and folder derives a UTF-8 byte key stored in a `BLOB` column:
1. **Unicode NFC Normalization:** Components are normalized to NFC (`unicode-normalization`). HFS+ stored names decomposed (NFD); APFS preserves input bytes and compares normalization-insensitively. NFD names appear on macOS from older volumes, archives, and some apps. Normalizing to NFC ensures equivalent names yield identical keys.
2. **Case Sensitivity Policy:**
   - **Case-insensitive volumes:** Components are lowercased using `str::to_lowercase()` after NFC. `A.JPG` and `a.jpg` yield the same key.
   - **Case-sensitive volumes:** Original casing is preserved after NFC.
3. **Separator Independence:** Components are joined strictly by forward slashes (`/`).

### Limits of Lowercasing
`str::to_lowercase()` is lowercasing, not full case-folding (`ß` remains `ß` rather than `ss`), and filesystems like NTFS and APFS use proprietary case tables. Furthermore, Rust's `to_lowercase` maps `I` to `i` in all locales (Turkish rules would yield dotless `ı`), and `İ` (U+0130) to `i` plus combining dot (U+0307). The key is a conservative approximation; the volume decides in the end, and collisions are reported rather than merged.

---

## 4. Key Collisions and Uniform Hierarchy (D27)

- **Collision handling:** SQLite enforces `UNIQUE(root_id, path_key)` on `folders` and `files`. Collisions return `CatalogueError::PathKeyCollision` with the existing ID, reported as a warning.
- **Root-folder row:** Every library root contains exactly one root-folder row (`parent_id = NULL`, empty path and key). Top-level folders reference it; every physical file has a non-NULL `folder_id`.

---

## 5. Catalogue Platform Guard (D26)

Because macOS and Windows store native paths in different byte encodings, opening a catalogue database on a different OS would corrupt paths.
When opening a database, `init_pool` follows **look, refuse, only then touch**:
- If `app_settings` exists, `native_path_encoding` is validated *before* any migration runs. A mismatched OS returns `CatalogueError::PlatformMismatch`, leaving tables untouched.
- On a fresh database, migrations run first, then the platform guard stamps the current timestamp.

---

## 6. Library Roots (D29)

A library root represents a top-level user folder registered for indexing.

### Canonical Form
Roots are stored in canonical form via `std::fs::canonicalize`, with all symlinks resolved:
- **Existence & containment:** The target must exist, be an absolute path, and reference a directory.
- **Unicode enforcement:** The path must be valid UTF-8. (Portability rules on backslashes and trailing colons apply to file and directory names under a root, not to the root's own path, D29j).
- **Windows verbatim prefixes:** The verbatim prefix (`\\?\` or `\\?\UNC\`) is preserved in `path_native` so paths exceeding the legacy 260-character `MAX_PATH` limit function correctly. For UI presentation, the prefix is stripped in `path_display` (`\\?\C:\x` -> `C:\x`, `\\?\UNC\srv\share` -> `\\srv\share`).

### Overlap Rule
Two roots must not be equal, nested within each other, or contain each other. Overlap comparison is performed component-wise over canonical paths, with each component NFC-normalized and lowercased (`to_lowercase()`), regardless of volume case-sensitivity:
- **Component-wise:** Sibling folders sharing a string prefix (e.g. `/photos` and `/photos2`) do not overlap and are permitted.
- **Status independence:** Overlap checks evaluate against all registered roots regardless of status (`active`, `paused`, `offline`).
- **Protected folders:** Roots cannot point to protected locations (the root of the system volume containing home or app-data, home directories, `/Users`, `C:\Users`, `/Volumes`, OS system folders, or the application data directory and anything inside or containing it; non-system drives like `D:\` or `/Volumes/Photos` and UNC shares are allowed as long as they do not contain a protected folder).
