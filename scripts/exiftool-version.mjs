export const PINNED_EXIFTOOL_VERSION = "13.61";
export const PINNED_EXIFTOOL_SHA256_MAC = "df7f42c14b4f58a60bec39feae0ee9593000ee27e44920d2b0a21c6627de0baa";
export const PINNED_EXIFTOOL_SHA256_WIN = "AB75B37515385A753AA60A58F68FFFFE1FF81E43B319DF9540978FB19E0580E5";

/**
 * Compares an extracted ExifTool version string against the pinned version.
 *
 * @param {string|null|undefined} rawOutput - Output from `exiftool -ver`
 * @param {boolean} isCi - Whether running in CI environment
 * @returns {{ ok: boolean, version: string|null, message: string, shouldFail: boolean }}
 */
export function checkExifToolVersion(rawOutput, isCi = Boolean(process.env.CI)) {
  const version = rawOutput ? rawOutput.trim().match(/^\d+(\.\d+)?/)?.[0] : null;

  if (!version) {
    const message = `Unable to parse ExifTool version from output: "${rawOutput || ""}"`;
    return {
      ok: false,
      version: null,
      message,
      shouldFail: isCi,
    };
  }

  if (version === PINNED_EXIFTOOL_VERSION) {
    return {
      ok: true,
      version,
      message: `Staged ExifTool version matches pinned version ${PINNED_EXIFTOOL_VERSION}`,
      shouldFail: false,
    };
  }

  const fixInstructions = `
================================================================================
⚠️ EXIFTOOL VERSION MISMATCH:
   Expected pinned version: ${PINNED_EXIFTOOL_VERSION}
   Found staged version:   ${version}

Fix for local development:
  macOS:
    curl -fL -o /tmp/exiftool.tar.gz https://downloads.sourceforge.net/project/exiftool/Image-ExifTool-${PINNED_EXIFTOOL_VERSION}.tar.gz
    echo "${PINNED_EXIFTOOL_SHA256_MAC}  /tmp/exiftool.tar.gz" | shasum -a 256 -c -
    mkdir -p deps/exiftool
    tar -xzf /tmp/exiftool.tar.gz -C deps/exiftool --strip-components=1

  Windows:
    curl.exe -fL -o exiftool.zip https://downloads.sourceforge.net/project/exiftool/exiftool-${PINNED_EXIFTOOL_VERSION}_64.zip
    # Verify SHA-256: ${PINNED_EXIFTOOL_SHA256_WIN}
    tar.exe -xf exiftool.zip -C deps/exiftool --strip-components=1
================================================================================
`.trim();

  return {
    ok: false,
    version,
    message: fixInstructions,
    shouldFail: isCi,
  };
}
