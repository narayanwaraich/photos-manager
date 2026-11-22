import { useCallback, useEffect, useState } from "react";
import {
  listRoots,
  addRoot,
  previewRemoveRoot,
  removeRoot,
  type RootSummary,
  type RemoveRootPreview,
} from "../lib/tauri";

function formatWarning(code: string): string {
  switch (code) {
    case "case_sensitivity_unknown":
      return "Could not tell whether this drive treats upper and lower case as different, so it is treated as case-insensitive.";
    case "network_volume":
      return "This folder is on a network drive. Scanning may be slow and the folder can go offline.";
    default:
      return `An unknown storage volume condition was detected (${code}).`;
  }
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
      // "cancelled" outcome is not an error; no action needed
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
        // Re-run the preview as mandated by decision h (do not retry removeRoot silently)
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
              <li key={w}>{formatWarning(w)}</li>
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
            return (
              <div
                key={root.id}
                style={{
                  display: "flex",
                  justifyContent: "space-between",
                  alignItems: "center",
                  padding: "0.75rem",
                  border: "1px solid #e0e0e0",
                  borderRadius: "4px",
                  background: root.reachable ? "#fafafa" : "#fff5f5",
                }}
              >
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

                <button
                  type="button"
                  onClick={() => void handleStartRemove(root)}
                  disabled={removing || isTarget}
                  style={{
                    padding: "0.4rem 0.8rem",
                    cursor: "pointer",
                    backgroundColor: "#fff",
                    border: "1px solid #ccc",
                    borderRadius: "4px",
                  }}
                >
                  {isTarget && !activePreview ? "Loading…" : "Remove"}
                </button>
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}
