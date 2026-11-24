import { useCallback, useEffect, useState } from "react";
import {
  listRoots,
  addRoot,
  previewRemoveRoot,
  removeRoot,
  startScan,
  cancelScan,
  type RootSummary,
  type RemoveRootPreview,
  type ScanEvent,
  ScanEventSchema,
} from "../lib/tauri";
import {
  formatRootWarning,
  formatScanWarning,
  formatScanningProgress,
  formatCompletedSummary,
  SCAN_CANCELLED_TEXT,
  SCAN_INTERRUPTED_TEXT,
  SCAN_PROGRESS_ERROR_TEXT,
} from "../lib/scanText";

interface ActiveScanState {
  jobId: string;
  filesSeen: number;
  foldersSeen: number;
  parseError: boolean;
}

interface ScanOutcomeState {
  kind: "completed" | "cancelled" | "failed";
  summaryText?: string;
  warnings?: Array<{ code: string; relPath: string }>;
  errorMessage?: string;
}

export function LibraryFolders() {
  const [roots, setRoots] = useState<RootSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [adding, setAdding] = useState(false);
  const [generalError, setGeneralError] = useState<string | null>(null);
  const [latestWarnings, setLatestWarnings] = useState<string[]>([]);

  // Count-and-confirm removal state (spec §3.5, Decision h)
  const [previewingRootId, setPreviewingRootId] = useState<string | null>(null);
  const [activePreview, setActivePreview] = useState<RemoveRootPreview | null>(null);
  const [previewTargetLabel, setPreviewTargetLabel] = useState<string>("");
  const [removing, setRemoving] = useState(false);

  // Active scan sessions tracking live progress per root (Decision g, k)
  const [activeScans, setActiveScans] = useState<Record<string, ActiveScanState>>({});
  // Terminal outcomes per root for completed/cancelled/failed scans
  const [scanOutcomes, setScanOutcomes] = useState<Record<string, ScanOutcomeState>>({});
  // Cancelling state per root to disable repeated clicks
  const [cancellingRoots, setCancellingRoots] = useState<Record<string, boolean>>({});

  const fetchRoots = useCallback(async () => {
    setLoading(true);
    setGeneralError(null);
    const res = await listRoots();
    if (res.status === "ok") {
      setRoots(res.data);
    } else {
      setGeneralError(res.error.message);
    }
    setLoading(false);
  }, []);

  useEffect(() => {
    void fetchRoots();
  }, [fetchRoots]);

  // Window-reload reconciliation polling: refresh every 2s while any root has activeJobId (Decision j)
  useEffect(() => {
    const anyActive = roots.some((r) => r.activeJobId !== null);
    if (!anyActive) return;

    const timer = setInterval(() => {
      void fetchRoots();
    }, 2000);

    return () => clearInterval(timer);
  }, [roots, fetchRoots]);

  const handleAddFolder = async () => {
    setAdding(true);
    setGeneralError(null);
    setLatestWarnings([]);

    const res = await addRoot();
    if (res.status === "ok") {
      if (res.data.outcome === "added") {
        if (res.data.warnings.length > 0) {
          setLatestWarnings(res.data.warnings);
        }
        await fetchRoots();
      }
    } else {
      setGeneralError(res.error.message);
    }
    setAdding(false);
  };

  const handleStartRemove = async (root: RootSummary) => {
    setGeneralError(null);
    setPreviewingRootId(root.id);
    setPreviewTargetLabel(root.label);

    const res = await previewRemoveRoot(root.id);
    if (res.status === "ok") {
      setActivePreview(res.data);
    } else {
      setGeneralError(res.error.message);
      setActivePreview(null);
      setPreviewingRootId(null);
    }
  };

  const handleCancelRemove = () => {
    setActivePreview(null);
    setPreviewingRootId(null);
    setPreviewTargetLabel("");
  };

  const handleConfirmRemove = async () => {
    if (!activePreview) return;
    setRemoving(true);
    setGeneralError(null);

    const rootId = activePreview.rootId;
    const expectedFiles = activePreview.fileCount;

    const res = await removeRoot(rootId, expectedFiles);
    if (res.status === "ok") {
      setActivePreview(null);
      setPreviewingRootId(null);
      setPreviewTargetLabel("");
      await fetchRoots();
    } else {
      setGeneralError(res.error.message);
      if (res.error.code === "root_changed_since_preview") {
        const refreshPreview = await previewRemoveRoot(rootId);
        if (refreshPreview.status === "ok") {
          setActivePreview(refreshPreview.data);
        } else {
          setActivePreview(null);
          setPreviewingRootId(null);
        }
      } else {
        setActivePreview(null);
        setPreviewingRootId(null);
      }
    }
    setRemoving(false);
  };

  // Start scanning a library root
  const handleStartScan = async (root: RootSummary) => {
    setGeneralError(null);
    // Clear any previous outcome for this root
    setScanOutcomes((prev) => {
      const next = { ...prev };
      delete next[root.id];
      return next;
    });

    const res = await startScan(root.id, (event: ScanEvent) => {
      // Validate every event with Zod (Part D)
      const parseResult = ScanEventSchema.safeParse(event);
      if (!parseResult.success) {
        setActiveScans((prev) => {
          const current = prev[root.id];
          if (!current) return prev;
          return {
            ...prev,
            [root.id]: { ...current, parseError: true },
          };
        });
        return;
      }

      const validEvent = parseResult.data;

      switch (validEvent.kind) {
        case "started":
          setActiveScans((prev) => ({
            ...prev,
            [root.id]: {
              jobId: validEvent.jobId,
              filesSeen: 0,
              foldersSeen: 0,
              parseError: false,
            },
          }));
          break;

        case "progress":
          setActiveScans((prev) => {
            const current = prev[root.id];
            if (!current) return prev;
            return {
              ...prev,
              [root.id]: {
                ...current,
                filesSeen: validEvent.filesSeen,
                foldersSeen: validEvent.foldersSeen,
              },
            };
          });
          break;

        case "finished":
          setActiveScans((prev) => {
            const next = { ...prev };
            delete next[root.id];
            return next;
          });
          setCancellingRoots((prev) => {
            const next = { ...prev };
            delete next[root.id];
            return next;
          });

          if (validEvent.outcome === "completed") {
            setScanOutcomes((prev) => ({
              ...prev,
              [root.id]: {
                kind: "completed",
                summaryText: formatCompletedSummary(
                  validEvent.counts,
                  validEvent.warningCount,
                ),
                warnings: validEvent.warnings,
              },
            }));
          } else {
            setScanOutcomes((prev) => ({
              ...prev,
              [root.id]: {
                kind: "cancelled",
                summaryText: SCAN_CANCELLED_TEXT,
              },
            }));
          }
          // Refresh roots list to update fileCount, lastScanAtMs, etc.
          void fetchRoots();
          break;

        case "failed":
          setActiveScans((prev) => {
            const next = { ...prev };
            delete next[root.id];
            return next;
          });
          setCancellingRoots((prev) => {
            const next = { ...prev };
            delete next[root.id];
            return next;
          });
          setScanOutcomes((prev) => ({
            ...prev,
            [root.id]: {
              kind: "failed",
              errorMessage:
                validEvent.code === "interrupted"
                  ? SCAN_INTERRUPTED_TEXT
                  : `Scan failed (${validEvent.code}).`,
            },
          }));
          void fetchRoots();
          break;
      }
    });

    if (res.status === "ok") {
      setActiveScans((prev) => ({
        ...prev,
        [root.id]: {
          jobId: res.data.jobId,
          filesSeen: 0,
          foldersSeen: 0,
          parseError: false,
        },
      }));
    } else {
      setGeneralError(res.error.message);
    }
  };

  // Cancel an in-flight scan
  const handleCancelScan = async (rootId: string, jobId: string) => {
    setCancellingRoots((prev) => ({ ...prev, [rootId]: true }));
    const res = await cancelScan(jobId);
    if (res.status !== "ok") {
      setGeneralError(res.error.message);
      setCancellingRoots((prev) => ({ ...prev, [rootId]: false }));
    }
  };

  return (
    <section style={{ marginTop: "1.5rem", padding: "1rem", border: "1px solid #ccc", borderRadius: "8px" }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: "1rem" }}>
        <h2 style={{ margin: 0 }}>Library Folders</h2>
        <div style={{ display: "flex", gap: "0.5rem" }}>
          <button
            type="button"
            onClick={() => void fetchRoots()}
            disabled={loading || adding || removing}
            style={{ padding: "0.5rem 1rem", cursor: "pointer" }}
          >
            {loading ? "Checking…" : "Refresh"}
          </button>
          <button
            type="button"
            onClick={() => void handleAddFolder()}
            disabled={adding || loading || removing}
            style={{ padding: "0.5rem 1rem", cursor: "pointer", fontWeight: "bold" }}
          >
            {adding ? "Opening Dialog…" : "Add folder…"}
          </button>
        </div>
      </div>

      {generalError && (
        <div
          role="alert"
          style={{
            padding: "0.75rem",
            marginBottom: "1rem",
            backgroundColor: "#fee",
            color: "#c00",
            border: "1px solid #fcc",
            borderRadius: "4px",
          }}
        >
          <strong>Error: </strong>
          <span>{generalError}</span>
        </div>
      )}

      {latestWarnings.length > 0 && (
        <div
          role="status"
          style={{
            padding: "0.75rem",
            marginBottom: "1rem",
            backgroundColor: "#fffbe6",
            color: "#8a6d3b",
            border: "1px solid #ffe58f",
            borderRadius: "4px",
          }}
        >
          <strong>Notice: </strong>
          <ul style={{ margin: "0.5rem 0 0 1.25rem", padding: 0 }}>
            {latestWarnings.map((w) => (
              <li key={w}>{formatRootWarning(w)}</li>
            ))}
          </ul>
        </div>
      )}

      {activePreview && (
        <div
          role="dialog"
          aria-labelledby="confirm-remove-heading"
          style={{
            padding: "1rem",
            marginBottom: "1rem",
            backgroundColor: "#fff0f0",
            border: "2px solid #e53e3e",
            borderRadius: "6px",
          }}
        >
          <h3 id="confirm-remove-heading" style={{ margin: "0 0 0.5rem 0", color: "#c53030" }}>
            Remove folder &ldquo;{previewTargetLabel}&rdquo;?
          </h3>
          <p style={{ margin: "0 0 1rem 0" }}>
            Remove this folder from the library? This deletes {activePreview.fileCount} files,{" "}
            {activePreview.folderCount} folders and {activePreview.assetCount} assets from the catalogue. Nothing on your
            disk is touched.
          </p>
          <div style={{ display: "flex", gap: "0.5rem" }}>
            <button
              type="button"
              onClick={handleCancelRemove}
              disabled={removing}
              style={{ padding: "0.4rem 0.8rem", cursor: "pointer" }}
            >
              Cancel
            </button>
            <button
              type="button"
              onClick={() => void handleConfirmRemove()}
              disabled={removing}
              style={{
                padding: "0.4rem 0.8rem",
                backgroundColor: "#e53e3e",
                color: "#fff",
                border: "none",
                borderRadius: "4px",
                cursor: "pointer",
                fontWeight: "bold",
              }}
            >
              {removing ? "Removing…" : "Remove"}
            </button>
          </div>
        </div>
      )}

      {roots.length === 0 && !loading && (
        <p style={{ color: "#666", fontStyle: "italic", margin: "1rem 0" }}>
          No folders added to the library yet. Click &ldquo;Add folder…&rdquo; to choose one.
        </p>
      )}

      {roots.length > 0 && (
        <div style={{ display: "flex", flexDirection: "column", gap: "0.75rem" }}>
          {roots.map((root) => {
            const isTarget = previewingRootId === root.id;
            const activeScan = activeScans[root.id];
            // Recovered from window reload if not in activeScans but activeJobId is present
            const isRunningFromReload = !activeScan && root.activeJobId !== null;
            const isScanning = Boolean(activeScan) || isRunningFromReload;
            const outcome = scanOutcomes[root.id];
            const isCancelling = Boolean(cancellingRoots[root.id]);

            // Scan button enablement rules (Decision k)
            const canScan = root.status === "Active" && root.reachable && !isScanning;
            let disabledReason: string | null = null;
            if (root.status !== "Active") {
              disabledReason = `Folder is ${root.status.toLowerCase()}`;
            } else if (!root.reachable) {
              disabledReason = "Folder is not reachable";
            }

            return (
              <div
                key={root.id}
                style={{
                  display: "flex",
                  flexDirection: "column",
                  padding: "0.75rem",
                  border: "1px solid #e0e0e0",
                  borderRadius: "4px",
                  background: root.reachable ? "#fafafa" : "#fff5f5",
                }}
              >
                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                  <div style={{ flex: 1, minWidth: 0, marginRight: "1rem" }}>
                    <div style={{ display: "flex", alignItems: "center", gap: "0.5rem", flexWrap: "wrap" }}>
                      <strong style={{ fontSize: "1.05rem" }}>{root.label}</strong>
                      {!root.reachable && (
                        <span
                          style={{
                            backgroundColor: "#fed7d7",
                            color: "#c53030",
                            padding: "0.1rem 0.4rem",
                            borderRadius: "3px",
                            fontSize: "0.8rem",
                            fontWeight: "bold",
                          }}
                        >
                          not reachable
                        </span>
                      )}
                      <span
                        style={{
                          backgroundColor: "#edf2f7",
                          color: "#4a5568",
                          padding: "0.1rem 0.4rem",
                          borderRadius: "3px",
                          fontSize: "0.8rem",
                        }}
                      >
                        {root.volumeKind}
                        {root.volumeLabel ? ` · ${root.volumeLabel}` : ""}
                      </span>
                    </div>
                    <div
                      style={{
                        fontFamily: "monospace",
                        fontSize: "0.85rem",
                        color: "#555",
                        marginTop: "0.25rem",
                        wordBreak: "break-all",
                      }}
                    >
                      {root.pathDisplay}
                    </div>
                    <div style={{ fontSize: "0.85rem", color: "#666", marginTop: "0.25rem" }}>
                      {root.fileCount} files · {root.folderCount} folders
                    </div>
                  </div>

                  <div style={{ display: "flex", gap: "0.5rem", alignItems: "center" }}>
                    {isScanning ? (
                      <button
                        type="button"
                        onClick={() => {
                          const jobId = activeScan?.jobId ?? root.activeJobId;
                          if (jobId) void handleCancelScan(root.id, jobId);
                        }}
                        disabled={isCancelling}
                        style={{
                          padding: "0.4rem 0.8rem",
                          cursor: "pointer",
                          backgroundColor: "#fff",
                          border: "1px solid #e53e3e",
                          color: "#c53030",
                          borderRadius: "4px",
                          fontWeight: "bold",
                        }}
                      >
                        {isCancelling ? "Cancelling…" : "Cancel"}
                      </button>
                    ) : (
                      <button
                        type="button"
                        onClick={() => void handleStartScan(root)}
                        disabled={!canScan || removing}
                        title={disabledReason ?? "Scan folder for photos"}
                        style={{
                          padding: "0.4rem 0.8rem",
                          cursor: canScan ? "pointer" : "not-allowed",
                          backgroundColor: canScan ? "#2b6cb0" : "#cbd5e0",
                          color: "#fff",
                          border: "none",
                          borderRadius: "4px",
                          fontWeight: "bold",
                        }}
                      >
                        Scan
                      </button>
                    )}

                    <button
                      type="button"
                      onClick={() => void handleStartRemove(root)}
                      disabled={removing || isTarget || isScanning}
                      style={{
                        padding: "0.4rem 0.8rem",
                        cursor: isScanning ? "not-allowed" : "pointer",
                        backgroundColor: "#fff",
                        border: "1px solid #ccc",
                        borderRadius: "4px",
                      }}
                    >
                      {isTarget && !activePreview ? "Loading…" : "Remove"}
                    </button>
                  </div>
                </div>

                {/* Visible reason when scan button is disabled */}
                {disabledReason && !isScanning && (
                  <div style={{ fontSize: "0.8rem", color: "#a0aec0", marginTop: "0.4rem" }}>
                    {disabledReason}
                  </div>
                )}

                {/* Live progress line while scanning */}
                {activeScan && (
                  <div
                    style={{
                      marginTop: "0.5rem",
                      padding: "0.4rem 0.6rem",
                      backgroundColor: "#ebf8ff",
                      border: "1px solid #bee3f8",
                      borderRadius: "4px",
                      fontSize: "0.85rem",
                      color: "#2b6cb0",
                    }}
                  >
                    <span>{formatScanningProgress(activeScan.filesSeen, activeScan.foldersSeen)}</span>
                    {activeScan.parseError && (
                      <span style={{ color: "#c53030", marginLeft: "0.5rem" }}>
                        ({SCAN_PROGRESS_ERROR_TEXT})
                      </span>
                    )}
                  </div>
                )}

                {/* Recovered running state after window reload */}
                {isRunningFromReload && (
                  <div
                    style={{
                      marginTop: "0.5rem",
                      padding: "0.4rem 0.6rem",
                      backgroundColor: "#ebf8ff",
                      border: "1px solid #bee3f8",
                      borderRadius: "4px",
                      fontSize: "0.85rem",
                      color: "#2b6cb0",
                    }}
                  >
                    A scan is running…
                  </div>
                )}

                {/* Completed summary line */}
                {outcome?.kind === "completed" && (
                  <div
                    style={{
                      marginTop: "0.5rem",
                      padding: "0.4rem 0.6rem",
                      backgroundColor: "#f0fff4",
                      border: "1px solid #c6f6d5",
                      borderRadius: "4px",
                      fontSize: "0.85rem",
                      color: "#22543d",
                    }}
                  >
                    <div>{outcome.summaryText}</div>
                    {outcome.warnings && outcome.warnings.length > 0 && (
                      <ul style={{ margin: "0.4rem 0 0 1rem", padding: 0, fontSize: "0.8rem", color: "#744210" }}>
                        {outcome.warnings.slice(0, 10).map((w, idx) => (
                          <li key={`${w.code}-${idx}`}>
                            {w.relPath ? `${w.relPath}: ` : ""}
                            {formatScanWarning(w.code)}
                          </li>
                        ))}
                      </ul>
                    )}
                  </div>
                )}

                {/* Cancelled summary line */}
                {outcome?.kind === "cancelled" && (
                  <div
                    style={{
                      marginTop: "0.5rem",
                      padding: "0.4rem 0.6rem",
                      backgroundColor: "#feebc8",
                      border: "1px solid #fbd38d",
                      borderRadius: "4px",
                      fontSize: "0.85rem",
                      color: "#744210",
                    }}
                  >
                    {outcome.summaryText}
                  </div>
                )}

                {/* Failed summary line */}
                {outcome?.kind === "failed" && (
                  <div
                    style={{
                      marginTop: "0.5rem",
                      padding: "0.4rem 0.6rem",
                      backgroundColor: "#fff5f5",
                      border: "1px solid #fed7d7",
                      borderRadius: "4px",
                      fontSize: "0.85rem",
                      color: "#c53030",
                    }}
                  >
                    {outcome.errorMessage}
                  </div>
                )}

                {/* Last job interrupted notice if no active outcome in this session */}
                {!isScanning && !outcome && root.lastJob?.errorCode === "interrupted" && (
                  <div
                    style={{
                      marginTop: "0.5rem",
                      padding: "0.4rem 0.6rem",
                      backgroundColor: "#fffaf0",
                      border: "1px solid #feebc8",
                      borderRadius: "4px",
                      fontSize: "0.85rem",
                      color: "#7b341e",
                    }}
                  >
                    {SCAN_INTERRUPTED_TEXT}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}
