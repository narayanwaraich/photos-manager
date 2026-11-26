# IPC Architecture & Command Authoring Guide

## 1. Decision D4: Fallback Taken (Hand-Written DTOs + Zod)

Per Decision D4 (spec §3.5) and TASK-1B, code generation via `tauri-specta` (2.0.0-rc.29) was evaluated first behind a small proof. The proof gate failed on two criteria:
1. Under `ErrorHandlingMode::Throw`, the error path is completely omitted from the generated TypeScript signatures (commands return `Promise<T>` rather than a typed error union), violating Proof Gate (c).
2. Under `ErrorHandlingMode::Result`, `tauri-specta` injects a runtime assertion (`const _assertTypedErrorFollowsContract: <T, E>(result: Promise<T>) => Promise<any> = typedError;`) containing an explicit `any`, an unused variable, and an unused type parameter `E`. This violates the strict TypeScript rules (`noUnusedLocals`, `noUnusedParameters`) and triggers `@typescript-eslint/no-explicit-any`, failing Proof Gate (b) ("the generated file passes pnpm typecheck (strict) and pnpm lint unchanged").

As specified by D4, the project adopted the robust fallback:
- **Rust backend:** Explicit DTO structs derived with `serde::{Serialize, Deserialize}` in `src-tauri/src/commands/`.
- **Frontend boundary:** Runtime validation using Zod schemas in `src/lib/tauri/schemas.ts`, inferring TypeScript types directly from schemas.
- **Error synchronization:** Stable error codes defined in `src/lib/tauri/errorCodes.ts` verified bidirectionally by a Rust test (`test_error_codes_sync_with_typescript`).
- **Typed results:** Commands return a discriminated union `{ status: "ok"; data: T } | { status: "error"; error: IpcError }`, keeping error paths fully typed on the frontend without unhandled promise rejections.

---

## 2. How to Add a Command (in 6 Steps)

1. **Define Rust DTOs:** In `src-tauri/src/commands/`, define input/output structs with `#[derive(Debug, Serialize, Deserialize)]` and `#[serde(rename_all = "camelCase")]`.
2. **Write plain service logic:** Implement an `async fn` taking plain parameters (e.g., `&SqlitePool`) returning `Result<YourDto, IpcError>`, and write a unit test without Tauri runtime.
3. **Expose the Tauri command:** Wrap the plain function in a thin `#[tauri::command]` function that extracts managed state (e.g., `tauri::State<'_, SqlitePool>`) and delegates.
4. **Register in Tauri handler:** Add your command to `tauri::generate_handler![...]` in `src-tauri/src/lib.rs`.
5. **Define Zod schemas:** In `src/lib/tauri/schemas.ts`, create the Zod schema (matching camelCase names) and export the inferred TypeScript type (`z.infer<typeof ...>`).
6. **Wrap in frontend client:** In `src/lib/tauri/index.ts`, export a typed helper that invokes the command and parses the output via `YourSchema.safeParse()`.

---

## 3. Asynchronous Job Streaming via Tauri Channel (`start_scan`, `cancel_scan`)

Long-running operations (such as filesystem scanning) return immediately across the IPC boundary with a job identifier, streaming continuous progress updates back to the UI through a Tauri `Channel` (spec §11, §12.1, TASK-1E decision g).

### Commands & Signatures

1. **`start_scan(rootId: string, onEvent: Channel<ScanEvent>) -> Result<StartScanResult, IpcError>`**
   - Returns `{ jobId: string }` immediately once the `jobs` row is persisted.
   - Streams `ScanEvent` items to `onEvent`.
2. **`cancel_scan(jobId: string) -> Result<(), IpcError>`**
   - Signals cancellation to the running scan and transitions the job row to `cancelling`.
   - Returns `job_not_found` if the job is not running or unknown.

### Event Shape (`ScanEvent`)

Tagged discriminated union on the `kind` field:
- `started`: `{ kind: "started", jobId: string, rootId: string }`
- `progress`: `{ kind: "progress", jobId: string, foldersSeen: number, filesSeen: number, batches: number }`
- `finished`: `{ kind: "finished", jobId: string, outcome: "completed" | "cancelled", counts: ScanCounts, warningCount: number, warnings: ScanWarning[], elapsedMs: number, recorded: boolean }`
- `failed`: `{ kind: "failed", jobId: string, code: string, recorded: boolean }`

### Decision (g) & TASK-1F Invariants
1. **Ordering:** `started` is always delivered first.
2. **Exclusivity:** Exactly one of `finished` or `failed` is delivered last, and no event follows it.
3. **Throttling:** `progress` events are throttled to at most four updates per second (250 ms ticker over a `watch` channel, latest value wins).
4. **Database persistence:** Job progress is persisted to SQLite at most once per second.
5. **Decoupled survival:** Failed delivery over the channel (e.g., WebView window reloaded or closed) is ignored and never aborts or halts the background scanning job.
6. **Bounded warnings:** `warnings` in `finished` contains at most the first 100 root-relative warnings (`{ code, relPath }`), never absolute paths.
7. **Honest terminal recording (risk 63, decision k):** Both `finished` and `failed` carry `recorded: boolean`. The final `finalize_job` + `prune_finished_jobs` transaction retries once after 200 ms on any error. The active-job entry in `JobManager` is removed only after that final write attempt completes and before the terminal event is emitted. If the retry still fails, `recorded` is `false`: the catalogue batches already committed remain intact, while the `jobs` row remains `running` until the next app start-up reconciles it as `interrupted` ("did not finish").

---

## 4. Root Availability & Relocation (`relocate_root`)

### Command Signature

- **`relocate_root(rootId: string) -> Result<RelocateRootResult, IpcError>`**
  - Runs the native folder dialog in Rust under `DialogGuard` (no path ever crosses from the WebView).
  - Returns `{ outcome: "cancelled" }` when the dialog is dismissed, or `{ outcome: "relocated", root: RootSummary }` when re-attached.
  - Enforces: root exists (`not_found`), stored status is `Offline` (`root_not_offline`), no scan active (`root_busy`), valid non-protected non-overlapping path (`root_path_invalid`, `root_not_unicode`, `root_protected`, `root_overlaps`), matching non-NULL volume identity (`relocate_volume_mismatch`), and content spot check of up to 30 `present`/`inaccessible` file rows (`relocate_content_mismatch`).
  - **Known limit:** Roots on volumes without an `fs_uuid` (for example some network shares) cannot be relocated in Phase 1. Two Windows volumes without GUIDs that share the same 8-hex volume serial cannot be distinguished by the availability volume guard.

### Pre-flight Availability & Walk Limit (decision e, risk 54)

- `list_roots` (2 s timeout) and `start_scan` / `scan_root` pre-flight (5 s timeout) run `is_dir()` and `describe_volume()` on a blocking thread with per-root in-flight deduplication (`ActiveReachableChecks`).
- **Known limit:** Once the directory walk inside `scan_root` starts, a single blocking filesystem call on a dead network share still cannot be cancelled until the OS call returns (risk 54).

### Additional Error Codes (TASK-1F)

- `root_offline`: Root is unreachable or check timed out / in flight at scan start.
- `root_volume_changed`: Root path resolves to a different volume `fs_uuid` than stored.
- `root_not_offline`: `relocate_root` called on a root whose stored status is not `Offline`.
- `relocate_volume_mismatch`: Relocate target has a different or missing `fs_uuid`.
- `relocate_content_mismatch`: Fewer than half of the sampled file rows exist under the relocate target.
- `volume_case_changed`: Reused volume changed case sensitivity while roots on that volume already hold file rows.
