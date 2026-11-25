import { Channel, invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import {
  AppInfoSchema,
  type AppInfoDto,
  IpcErrorSchema,
  type CommandResult,
  RootSummarySchema,
  type RootSummary,
  AddRootResultSchema,
  type AddRootResult,
  RemoveRootPreviewSchema,
  type RemoveRootPreview,
  StartScanResultSchema,
  type StartScanResult,
  ScanEventSchema,
  type ScanEvent,
} from "./schemas";

export * from "./errorCodes";
export * from "./schemas";

/**
 * Common invocation helper for all typed Tauri commands.
 * Dispatches via Tauri invoke, safely parses the result using Zod,
 * and maps any thrown IPC or runtime error into a typed CommandResult without throwing.
 */
async function invokeCommand<T>(
  cmd: string,
  args: Record<string, unknown> | undefined,
  schema: z.ZodType<T>,
): Promise<CommandResult<T>> {
  try {
    const raw = await invoke(cmd, args);
    const parsed = schema.safeParse(raw);
    if (!parsed.success) {
      return {
        status: "error",
        error: {
          code: "internal",
          message: `Malformed response from ${cmd}: ${parsed.error.message}`,
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

/**
 * Invokes the `get_app_info` Tauri command, validating the returned payload with Zod.
 */
export async function getAppInfo(): Promise<CommandResult<AppInfoDto>> {
  return invokeCommand("get_app_info", undefined, AppInfoSchema);
}

/**
 * Invokes the `list_roots` Tauri command, returning all library roots with volume details and reachable status.
 */
export async function listRoots(): Promise<CommandResult<RootSummary[]>> {
  return invokeCommand("list_roots", undefined, z.array(RootSummarySchema));
}

/**
 * Invokes the `add_root` Tauri command.
 * Note: Folder selection occurs in native Rust dialog; only an optional label crosses the boundary.
 */
export async function addRoot(label?: string): Promise<CommandResult<AddRootResult>> {
  return invokeCommand("add_root", label !== undefined ? { label } : undefined, AddRootResultSchema);
}

/**
 * Invokes the `preview_remove_root` Tauri command, retrieving item counts prior to confirmation.
 */
export async function previewRemoveRoot(rootId: string): Promise<CommandResult<RemoveRootPreview>> {
  return invokeCommand("preview_remove_root", { rootId }, RemoveRootPreviewSchema);
}

/**
 * Invokes the `remove_root` Tauri command with verified expected file count.
 */
export async function removeRoot(
  rootId: string,
  expectedFiles: number,
): Promise<CommandResult<null>> {
  return invokeCommand("remove_root", { rootId, expectedFiles }, z.null());
}

/**
 * Starts an asynchronous scan of the given library root, streaming progress events via a Tauri Channel.
 * Validates each event against ScanEventSchema. If parsing fails, invokes onInvalidEvent (if provided)
 * and drops the event without fabricating a failed terminal event (Decision g, R3).
 */
export async function startScan(
  rootId: string,
  onEvent: (event: ScanEvent) => void,
  onInvalidEvent?: () => void,
): Promise<CommandResult<StartScanResult>> {
  const channel = new Channel<unknown>();
  channel.onmessage = (message: unknown) => {
    const parsed = ScanEventSchema.safeParse(message);
    if (parsed.success) {
      onEvent(parsed.data);
    } else {
      onInvalidEvent?.();
    }
  };
  return invokeCommand("start_scan", { rootId, onEvent: channel }, StartScanResultSchema);
}

/**
 * Cancels a running scan job by its job ID.
 */
export async function cancelScan(jobId: string): Promise<CommandResult<null>> {
  return invokeCommand("cancel_scan", { jobId }, z.null());
}
