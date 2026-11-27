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

export const LastJobSummarySchema = z.object({
  jobId: z.string(),
  status: z.string(),
  errorCode: z.string().nullable(),
  finishedAtMs: z.number().int().nullable(),
});

export type LastJobSummary = z.infer<typeof LastJobSummarySchema>;

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
  lastScanAtMs: z.number().int().nullable(),
  lastJob: LastJobSummarySchema.nullable(),
  activeJobId: z.string().nullable(),
});

export type RootSummary = z.infer<typeof RootSummarySchema>;

export const StartScanResultSchema = z.object({
  jobId: z.string(),
});

export type StartScanResult = z.infer<typeof StartScanResultSchema>;

export const ScanCountsSchema = z.object({
  foldersSeen: z.number().int().nonnegative(),
  filesSeen: z.number().int().nonnegative(),
  new: z.number().int().nonnegative(),
  changed: z.number().int().nonnegative(),
  unchanged: z.number().int().nonnegative(),
  moved: z.number().int().nonnegative(),
  markedMissing: z.number().int().nonnegative(),
  markedInaccessible: z.number().int().nonnegative(),
  unsupported: z.number().int().nonnegative(),
  skippedByRule: z.number().int().nonnegative(),
});

export type ScanCounts = z.infer<typeof ScanCountsSchema>;

export const ScanWarningSchema = z.object({
  code: z.string(),
  relPath: z.string(),
});

export type ScanWarning = z.infer<typeof ScanWarningSchema>;

export const ScanEventSchema = z.discriminatedUnion("kind", [
  z.object({
    kind: z.literal("started"),
    jobId: z.string(),
    rootId: z.string(),
  }),
  z.object({
    kind: z.literal("progress"),
    jobId: z.string(),
    foldersSeen: z.number().int().nonnegative(),
    filesSeen: z.number().int().nonnegative(),
    batches: z.number().int().nonnegative(),
  }),
  z.object({
    kind: z.literal("finished"),
    jobId: z.string(),
    outcome: z.enum(["completed", "cancelled"]),
    counts: ScanCountsSchema,
    warningCount: z.number().int().nonnegative(),
    warnings: z.array(ScanWarningSchema),
    elapsedMs: z.number().int().nonnegative(),
    recorded: z.boolean(),
  }),
  z.object({
    kind: z.literal("failed"),
    jobId: z.string(),
    code: z.string(),
    recorded: z.boolean(),
  }),
]);

export type ScanEvent = z.infer<typeof ScanEventSchema>;

export const AddRootResultSchema = z.object({
  outcome: z.enum(["added", "cancelled"]),
  rootId: z.string().optional(),
  warnings: z.array(z.string()),
});

export type AddRootResult = z.infer<typeof AddRootResultSchema>;

export const RelocateRootResultSchema = z.object({
  outcome: z.enum(["relocated", "cancelled"]),
  root: RootSummarySchema.optional(),
});

export type RelocateRootResult = z.infer<typeof RelocateRootResultSchema>;

export const RemoveRootPreviewSchema = z.object({
  rootId: z.string(),
  fileCount: z.number().int(),
  folderCount: z.number().int(),
  assetCount: z.number().int(),
});

export type RemoveRootPreview = z.infer<typeof RemoveRootPreviewSchema>;
