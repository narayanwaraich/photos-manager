#!/usr/bin/env bash
# audit-zip.sh: Audit a Windows libvips zip archive against licensing and dependency rules.
#
# Usage: ./audit-zip.sh <path-to-vips-dev-*.zip>
#
# Rules:
#   (a) No file whose name contains "x265" anywhere in the archive.
#   (b) In every DLL EXCEPT libvips-42.dll and libvips-cpp-42.dll, strings -a finds no "x265" (case-insensitive).
#   (c) In ALL DLLs including those two, none of: libx265, x265_encoder, x265_param.
#       Note: The enum nick "x265" alone in libvips-42.dll is allowed because libvips
#       registers the VipsForeignHeifEncoder enum unconditionally in GObject introspection
#       (as verified in official upstream stock vips-dev-x64-web-8.18.6.zip). Any real
#       linkage to x265 imports/exports functions like libx265, x265_encoder_*, x265_param_*.
#   (d) bin/libde265.dll exists and libheif.dll contains the string "libde265".
#   (e) versions.json names libde265.
#   (f) No fftw, imagemagick, poppler (files or versions.json).

set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 <path-to-vips-dev-*.zip>" >&2
  exit 2
fi

ZIP_FILE="$1"
if [[ ! -f "$ZIP_FILE" ]]; then
  echo "Error: Archive file not found: $ZIP_FILE" >&2
  exit 2
fi

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

unzip -q "$ZIP_FILE" -d "$TMP_DIR"

VIPS_ROOT=$(find "$TMP_DIR" -mindepth 1 -maxdepth 1 -type d | head -n 1)
if [[ -z "$VIPS_ROOT" ]]; then
  VIPS_ROOT="$TMP_DIR"
fi

EXIT_CODE=0

echo "=== Auditing libvips archive: $(basename "$ZIP_FILE") ==="

# (a) No file whose name contains x265 anywhere in the zip
if find "$TMP_DIR" -iname "*x265*" | grep -q .; then
  echo "FAIL (a): Found file containing 'x265' in filename:"
  find "$TMP_DIR" -iname "*x265*"
  EXIT_CODE=1
else
  echo "PASS (a): No file containing 'x265' in name anywhere in archive"
fi

# (b) In every DLL EXCEPT libvips-42.dll and libvips-cpp-42.dll, strings -a finds no "x265" (case-insensitive)
rule_b_fail=0
while IFS= read -r -d '' dll; do
  base=$(basename "$dll")
  if [[ "$base" == "libvips-42.dll" || "$base" == "libvips-cpp-42.dll" ]]; then
    continue
  fi
  if strings -a "$dll" | grep -i -q "x265"; then
    echo "  Non-exempt DLL $base contains string 'x265'"
    rule_b_fail=1
  fi
done < <(find "$VIPS_ROOT" -type f -name "*.dll" -print0)

if [[ $rule_b_fail -ne 0 ]]; then
  echo "FAIL (b): Found 'x265' string in non-exempt DLL"
  EXIT_CODE=1
else
  echo "PASS (b): No 'x265' string found in any non-exempt DLL"
fi

# (c) In ALL DLLs including those two, none of: libx265, x265_encoder, x265_param
rule_c_fail=0
while IFS= read -r -d '' dll; do
  base=$(basename "$dll")
  if strings -a "$dll" | grep -E -i -q "libx265|x265_encoder|x265_param"; then
    echo "  DLL $base contains forbidden x265 symbol/linkage string"
    rule_c_fail=1
  fi
done < <(find "$VIPS_ROOT" -type f -name "*.dll" -print0)

if [[ $rule_c_fail -ne 0 ]]; then
  echo "FAIL (c): Found forbidden x265 linkage string (libx265, x265_encoder, x265_param)"
  EXIT_CODE=1
else
  echo "PASS (c): No forbidden x265 linkage strings (libx265, x265_encoder, x265_param) found in any DLL"
fi

# (d) bin/libde265.dll exists and libheif.dll contains the string "libde265"
rule_d_fail=0
if [[ ! -f "$VIPS_ROOT/bin/libde265.dll" && ! -f "$VIPS_ROOT/bin/libde265-0.dll" ]]; then
  echo "  libde265 DLL not found in bin/"
  rule_d_fail=1
fi
libheif_dll=$(find "$VIPS_ROOT/bin" -type f -name "libheif*.dll" | head -n 1)
if [[ -z "$libheif_dll" ]]; then
  echo "  libheif DLL not found in bin/"
  rule_d_fail=1
elif ! strings -a "$libheif_dll" | grep -i -q "libde265"; then
  echo "  libheif DLL ($libheif_dll) does not contain string 'libde265'"
  rule_d_fail=1
fi

if [[ $rule_d_fail -ne 0 ]]; then
  echo "FAIL (d): libde265 DLL missing or libheif DLL does not reference libde265"
  EXIT_CODE=1
else
  echo "PASS (d): bin/libde265.dll exists and libheif.dll contains string 'libde265'"
fi

# (e) versions.json names libde265
if [[ ! -f "$VIPS_ROOT/versions.json" ]]; then
  echo "FAIL (e): versions.json not found"
  EXIT_CODE=1
elif ! grep -q "libde265" "$VIPS_ROOT/versions.json"; then
  echo "FAIL (e): versions.json lacks 'libde265'"
  EXIT_CODE=1
else
  echo "PASS (e): versions.json names libde265"
fi

# (f) No fftw, imagemagick, poppler (files or versions.json)
rule_f_fail=0
if find "$TMP_DIR" -iname "*fftw*" -o -iname "*magick*" -o -iname "*poppler*" | grep -q .; then
  echo "  Found disallowed files (fftw/magick/poppler)"
  rule_f_fail=1
fi
if [[ -f "$VIPS_ROOT/versions.json" ]] && grep -E -i -q "fftw|imagemagick|poppler" "$VIPS_ROOT/versions.json"; then
  echo "  versions.json contains disallowed component (fftw/imagemagick/poppler)"
  rule_f_fail=1
fi

if [[ $rule_f_fail -ne 0 ]]; then
  echo "FAIL (f): Disallowed components (fftw, imagemagick, poppler) detected"
  EXIT_CODE=1
else
  echo "PASS (f): No fftw, imagemagick, or poppler components found in files or versions.json"
fi

if [[ $EXIT_CODE -eq 0 ]]; then
  echo "OVERALL: AUDIT PASSED"
else
  echo "OVERALL: AUDIT FAILED"
fi

exit $EXIT_CODE
