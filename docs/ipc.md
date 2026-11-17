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
