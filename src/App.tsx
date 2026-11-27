import { useEffect, useState } from "react";
import { getAppInfo, type AppInfoDto } from "./lib/tauri";
import { LibraryFolders } from "./components/LibraryFolders";

const STARTUP_RETRY_DELAY_MS = 100;
const STARTUP_MAX_RETRIES = 50;

function isUnmanagedStateError(message: string): boolean {
  const lower = message.toLowerCase();
  return lower.includes("state not managed") || lower.includes("before manage");
}

export default function App() {
  const [thumbLoaded, setThumbLoaded] = useState(false);
  const [thumbError, setThumbError] = useState<string | null>(null);

  const [appInfo, setAppInfo] = useState<AppInfoDto | null>(null);
  const [appInfoError, setAppInfoError] = useState<string | null>(null);

  // URL shape differs between macOS (WKWebView) and Windows (WebView2):
  // macOS / WebKit: asset-thumb://localhost/<id>
  // Windows / WebView2: http://asset-thumb.localhost/<id>
  const isWindows = typeof navigator !== "undefined" && navigator.userAgent.includes("Windows");
  const thumbUrl = isWindows
    ? "http://asset-thumb.localhost/test-thumb"
    : "asset-thumb://localhost/test-thumb";

  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;

    const loadAppInfo = async (attempt: number) => {
      const res = await getAppInfo();
      if (!active) return;
      if (res.status === "ok") {
        setAppInfo(res.data);
        return;
      }

      if (isUnmanagedStateError(res.error.message)) {
        if (attempt < STARTUP_MAX_RETRIES) {
          timer = setTimeout(() => {
            void loadAppInfo(attempt + 1);
          }, STARTUP_RETRY_DELAY_MS);
          return;
        }
        // Never show raw Tauri "state not managed" text in the UI (risk 55).
        setAppInfoError("The application could not finish starting. Please restart it.");
        return;
      }

      setAppInfoError(`${res.error.code}: ${res.error.message}`);
    };

    void loadAppInfo(0);
    return () => {
      active = false;
      if (timer !== undefined) {
        clearTimeout(timer);
      }
    };
  }, []);

  if (!appInfo && !appInfoError) {
    return (
      <main style={{ padding: "2rem", fontFamily: "system-ui, sans-serif" }}>
        <h1>Photos Manager</h1>
        <p>Starting…</p>
      </main>
    );
  }

  return (
    <main style={{ padding: "2rem", fontFamily: "system-ui, sans-serif" }}>
      <h1>Photos Manager</h1>
      <p style={{ color: "#666" }}>
        Phase 0 Spike · Tauri 2 + React + TypeScript + Vite
      </p>

      <section style={{ marginTop: "1.5rem", padding: "1rem", border: "1px solid #ccc", borderRadius: "8px" }}>
        <h2>App Info (IPC Proof)</h2>
        {appInfo && (
          <div>
            <p><strong>App Version:</strong> {appInfo.appVersion}</p>
            <p><strong>Platform Encoding:</strong> {appInfo.platformEncoding}</p>
            <p><strong>Migrations Applied:</strong> {appInfo.migrationsApplied}</p>
          </div>
        )}
        {appInfoError && (
          <p style={{ color: "red" }}>Error loading app info: {appInfoError}</p>
        )}
      </section>

      {appInfo && <LibraryFolders />}

      <section style={{ marginTop: "1.5rem", padding: "1rem", border: "1px solid #ccc", borderRadius: "8px" }}>
        <h2>Custom Protocol &amp; CSP Spike</h2>
        <p>Serving thumbnail from custom scheme: <code>{thumbUrl}</code></p>
        <div style={{ marginTop: "1rem" }}>
          <img
            src={thumbUrl}
            alt="Spike Thumbnail"
            width={320}
            height={240}
            style={{ border: "1px solid #ddd", borderRadius: "4px", objectFit: "contain", background: "#f5f5f5" }}
            onLoad={() => setThumbLoaded(true)}
            onError={() => setThumbError("Failed to load thumbnail via custom protocol")}
          />
        </div>
        {thumbLoaded && <p style={{ color: "green" }}>✓ Thumbnail loaded successfully via custom protocol.</p>}
        {thumbError && <p style={{ color: "red" }}>✗ {thumbError}</p>}
      </section>
    </main>
  );
}
