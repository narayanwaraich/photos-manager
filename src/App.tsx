import { useState } from "react";

export default function App() {
  const [thumbLoaded, setThumbLoaded] = useState(false);
  const [thumbError, setThumbError] = useState<string | null>(null);

  // URL shape differs between macOS (WKWebView) and Windows (WebView2):
  // macOS / WebKit: asset-thumb://localhost/<id>
  // Windows / WebView2: http://asset-thumb.localhost/<id>
  const isWindows = typeof navigator !== "undefined" && navigator.userAgent.includes("Windows");
  const thumbUrl = isWindows
    ? "http://asset-thumb.localhost/test-thumb"
    : "asset-thumb://localhost/test-thumb";

  return (
    <main style={{ padding: "2rem", fontFamily: "system-ui, sans-serif" }}>
      <h1>Photos Manager</h1>
      <p style={{ color: "#666" }}>
        Phase 0 Spike · Tauri 2 + React + TypeScript + Vite
      </p>

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
