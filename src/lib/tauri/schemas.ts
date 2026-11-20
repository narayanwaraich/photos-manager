import { z } from "zod";
import { KNOWN_IPC_ERROR_CODES } from "./errorCodes";

export const AppInfoSchema = z.object({
  appVersion: z.string(),
  platformEncoding: z.string(),
  migrationsApplied: z.number().int().nonnegative(),
});

export type AppInfoDto = z.infer<typeof AppInfoSchema>;

export const IpcErrorSchema = z.object({
  code: z.enum(KNOWN_IPC_ERROR_CODES).or(z.string()),
  message: z.string(),
});

export type IpcError = z.infer<typeof IpcErrorSchema>;

export type CommandResult<T> =
  | { status: "ok"; data: T }
  | { status: "error"; error: IpcError };
