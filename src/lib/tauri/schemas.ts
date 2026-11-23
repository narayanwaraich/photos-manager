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

export const VolumeKindSchema = z.enum([
  "Internal",
  "Removable",
  "Network",
  "Unknown",
]);

export type VolumeKind = z.infer<typeof VolumeKindSchema>;

export const RootStatusSchema = z.enum([
  "Active",
  "Paused",
  "Offline",
  "Removed",
]);

export type RootStatus = z.infer<typeof RootStatusSchema>;

export const RootSummarySchema = z.object({
  id: z.string(),
  label: z.string(),
  pathDisplay: z.string(),
  volumeKind: VolumeKindSchema,
  volumeLabel: z.string().nullable(),
  volumeCaseSensitive: z.boolean(),
  status: RootStatusSchema,
  reachable: z.boolean(),
  fileCount: z.number().int(),
  folderCount: z.number().int(),
});

export type RootSummary = z.infer<typeof RootSummarySchema>;

export const AddRootResultSchema = z.object({
  outcome: z.enum(["added", "cancelled"]),
  rootId: z.string().optional(),
  warnings: z.array(z.string()),
});

export type AddRootResult = z.infer<typeof AddRootResultSchema>;

export const RemoveRootPreviewSchema = z.object({
  rootId: z.string(),
  fileCount: z.number().int(),
  folderCount: z.number().int(),
  assetCount: z.number().int(),
});

export type RemoveRootPreview = z.infer<typeof RemoveRootPreviewSchema>;
