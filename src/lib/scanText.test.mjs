import test from "node:test";
import assert from "node:assert/strict";
import {
  AUTO_SCAN_CANCELLED_BODY,
  AUTO_SCAN_COMPLETED_BODY,
  format24HourTimeFromParts,
  formatAutoScanResultLine,
  formatLocalTime24,
  getReachabilityLabel,
} from "./scanText.ts";

test("format24HourTimeFromParts pads single-digit hours, minutes, and seconds", () => {
  assert.equal(format24HourTimeFromParts({ hours: 4, minutes: 7, seconds: 9 }), "04:07:09");
  assert.equal(format24HourTimeFromParts({ hours: 14, minutes: 32, seconds: 5 }), "14:32:05");
});

test("formatAutoScanResultLine formats fixed timestamp in deterministic 24-hour time", () => {
  // 2024-11-24T14:32:05.000Z
  const ts = Date.UTC(2024, 10, 24, 14, 32, 5);
  const utcParts = (d) => ({
    hours: d.getUTCHours(),
    minutes: d.getUTCMinutes(),
    seconds: d.getUTCSeconds(),
  });

  assert.equal(
    formatAutoScanResultLine(AUTO_SCAN_COMPLETED_BODY, ts, utcParts),
    "Started automatically at 14:32:05: scan completed.",
  );
  assert.equal(
    formatAutoScanResultLine(AUTO_SCAN_CANCELLED_BODY, ts, utcParts),
    "Started automatically at 14:32:05: scan cancelled. Nothing was marked missing.",
  );
});

test("formatLocalTime24 and formatAutoScanResultLine handle null or invalid timestamps", () => {
  assert.equal(formatLocalTime24(Number.NaN), null);
  assert.equal(
    formatAutoScanResultLine(AUTO_SCAN_COMPLETED_BODY, null),
    "Started automatically: scan completed.",
  );
});

test("getReachabilityLabel shows Checking... for Active+unreachable and not reachable for Offline", () => {
  assert.equal(getReachabilityLabel("Active", true), null);
  assert.equal(getReachabilityLabel("Active", false), "Checking...");
  assert.equal(getReachabilityLabel("Offline", false), "not reachable");
  assert.equal(getReachabilityLabel("Offline", true), null);
  assert.equal(getReachabilityLabel("Paused", false), "not reachable");
  assert.equal(getReachabilityLabel("Removed", false), "not reachable");
});
