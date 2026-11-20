import { invoke } from "@tauri-apps/api/core";
import { AppInfoSchema, type AppInfoDto, IpcErrorSchema, type CommandResult } from "./schemas";

export * from "./errorCodes";
export * from "./schemas";

/**
 * Invokes the `get_app_info` Tauri command, validating the returned payload with Zod.
 * Returns a typed Result object so error paths are explicitly typed without throwing.
 */
export async function getAppInfo(): Promise<CommandResult<AppInfoDto>> {
  try {
    const raw = await invoke("get_app_info");
    const parsed = AppInfoSchema.safeParse(raw);
    if (!parsed.success) {
      return {
        status: "error",
        error: {
          code: "internal",
          message: `Malformed response from get_app_info: ${parsed.error.message}`,
        },
      };
    }
    return { status: "ok", data: parsed.data };
  } catch (err: unknown) {
    const parsedError = IpcErrorSchema.safeParse(err);
    if (parsedError.success) {
      return { status: "error", error: parsedError.data };
    }
    return {
      status: "error",
      error: {
        code: "internal",
        message: err instanceof Error ? err.message : String(err),
      },
    };
  }
}
