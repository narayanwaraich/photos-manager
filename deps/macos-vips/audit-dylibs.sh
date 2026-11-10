#!/usr/bin/env bash
# audit-dylibs.sh: Audit a macOS arm64 libvips zip archive (or unpacked directory)
# against licensing, codec exclusion, and relocatable @rpath rules.
#
# Usage: ./audit-dylibs.sh <path-to-vips-macos-arm64-*.zip | unpacked-dir>
#
# Rules enforced:
#   (a) No file whose name contains x265, x264, fftw, imagequant, magick, poppler, rsvg, openslide.
#   (b) Relocatable Mach-O check via otool -L across all dylibs and CLI binaries:
#       - Every dylib's LC_ID_DYLIB must start with @rpath/
#       - Zero non-system absolute paths (/opt/homebrew, /usr/local, /Cellar, /Users, /tmp, /var)
#       - Every @rpath/<lib> dependency resolves to an actual file inside lib/
#       - Zero forbidden otool -L dependency names (x265, x264, fftw, imagequant, magick, poppler, rsvg, openslide)
#   (c) Binary string/symbol check across ALL Mach-O files:
#       - strings -a must never find libx265, x265_encoder, x265_param anywhere
#       - nm -g must never find x265 symbol anywhere
#       - strings -a must never find bare "x265" in any file EXCEPT libvips*.dylib
#         (libvips compiles VipsForeignHeifEncoder/Compression enum nick "x265" from foreign.h unconditionally)
#   (d) libde265 is present in lib/ AND dynamically linked into libheif via otool -L.
#   (e) versions.json exists, names libde265, accounts for all regular .dylib files in lib/,
#       contains no forbidden components, no "unknown" licenses, and no non-LGPL GPL licenses.

set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "Usage: $0 <path-to-zip-or-dir>" >&2
  exit 2
fi

INPUT_PATH="$1"
TMP_DIR=""
# shellcheck disable=SC2329
cleanup() {
  if [[ -n "$TMP_DIR" ]] && [[ -d "$TMP_DIR" ]]; then
    rm -rf "$TMP_DIR"
  fi
}
trap cleanup EXIT

if [[ -f "$INPUT_PATH" ]]; then
  TMP_DIR="$(mktemp -d)"
  unzip -q "$INPUT_PATH" -d "$TMP_DIR"
  VIPS_ROOT="$(find "$TMP_DIR" -mindepth 1 -maxdepth 1 -type d | head -n 1)"
  if [[ -z "$VIPS_ROOT" ]]; then
    VIPS_ROOT="$TMP_DIR"
  fi
  AUDIT_TARGET="$(basename "$INPUT_PATH")"
elif [[ -d "$INPUT_PATH" ]]; then
  VIPS_ROOT="$INPUT_PATH"
  AUDIT_TARGET="$(basename "$INPUT_PATH")"
else
  echo "Error: Archive or directory not found: $INPUT_PATH" >&2
  exit 2
fi

EXIT_CODE=0
FORBIDDEN_PATTERN="x265|x264|fftw|imagequant|magick|poppler|rsvg|openslide"

echo "=== Auditing macOS libvips archive: $AUDIT_TARGET ==="

# (a) Forbidden filenames anywhere in archive
if find "$VIPS_ROOT" \( -iname "*x265*" -o -iname "*x264*" -o -iname "*fftw*" -o -iname "*imagequant*" -o -iname "*magick*" -o -iname "*poppler*" -o -iname "*rsvg*" -o -iname "*openslide*" \) | grep -q .; then
  echo "FAIL (a): Found forbidden filename matching (${FORBIDDEN_PATTERN}):"
  find "$VIPS_ROOT" \( -iname "*x265*" -o -iname "*x264*" -o -iname "*fftw*" -o -iname "*imagequant*" -o -iname "*magick*" -o -iname "*poppler*" -o -iname "*rsvg*" -o -iname "*openslide*" \)
  EXIT_CODE=1
else
  echo "PASS (a): No forbidden filenames (${FORBIDDEN_PATTERN}) in archive"
fi

# Collect Mach-O dylibs (regular files) and executables in bin/
MACHO_FILES=()
if [[ -d "$VIPS_ROOT/lib" ]]; then
  while IFS= read -r -d '' f; do
    MACHO_FILES+=("$f")
  done < <(find "$VIPS_ROOT/lib" -maxdepth 1 -type f -name "*.dylib" -print0)
fi
if [[ -d "$VIPS_ROOT/bin" ]]; then
  while IFS= read -r -d '' f; do
    if file -b "$f" | grep -q "Mach-O"; then
      MACHO_FILES+=("$f")
    fi
  done < <(find "$VIPS_ROOT/bin" -maxdepth 1 -type f -print0)
fi

if [[ ${#MACHO_FILES[@]} -eq 0 ]]; then
  echo "FAIL (b): No Mach-O binaries or dylibs found under $VIPS_ROOT/{lib,bin}"
  exit 1
fi

# (b) Relocatable otool -L audit: ID @rpath/, zero non-system absolute paths, all @rpath resolve locally, no forbidden linkages
rule_b_fail=0
for f in "${MACHO_FILES[@]}"; do
  base="$(basename "$f")"
  if [[ "$base" == *.dylib ]]; then
    id_line="$(otool -D "$f" | sed -n '2p' | tr -d '[:space:]')"
    if [[ "$id_line" != "@rpath/"* ]]; then
      echo "  Dylib $base has non-@rpath LC_ID_DYLIB: '$id_line'"
      rule_b_fail=1
    fi
  fi

  # Skip header line (1) and dylib self-ID line (2 for .dylib)
  if [[ "$base" == *.dylib ]]; then
    deps="$(otool -L "$f" | tail -n +3)"
  else
    deps="$(otool -L "$f" | tail -n +2)"
  fi

  while IFS= read -r line; do
    [[ -z "$line" ]] && continue
    dep="${line#"${line%%[![:space:]]*}"}"
    dep="${dep%% *}"
    if [[ "$dep" =~ $FORBIDDEN_PATTERN ]]; then
      echo "  $base links forbidden dependency: $dep"
      rule_b_fail=1
    fi
    if [[ "$dep" == "/System/"* ]] || [[ "$dep" == "/usr/lib/"* ]]; then
      continue
    elif [[ "$dep" == "@rpath/"* ]]; then
      rel_lib="${dep#@rpath/}"
      if [[ ! -e "$VIPS_ROOT/lib/$rel_lib" ]]; then
        echo "  $base references missing bundled @rpath library: $dep"
        rule_b_fail=1
      fi
    else
      echo "  $base has non-system, non-@rpath dependency path: $dep"
      rule_b_fail=1
    fi
  done <<< "$deps"
done

if [[ $rule_b_fail -ne 0 ]]; then
  echo "FAIL (b): Relocatable otool -L or forbidden dependency check failed"
  EXIT_CODE=1
else
  echo "PASS (b): All ${#MACHO_FILES[@]} Mach-O files use @rpath/ cleanly with zero Homebrew/local path leaks or forbidden linkages"
fi

# (c) Strings & nm symbols audit for x265 (strict linkage check + documented Lesson-6 enum nick exemption on libvips*.dylib)
rule_c_fail=0
for f in "${MACHO_FILES[@]}"; do
  base="$(basename "$f")"
  if strings -a "$f" | grep -E -i -q "libx265|x265_encoder|x265_param"; then
    echo "  $base contains forbidden x265 symbol/linkage string (libx265/x265_encoder/x265_param)"
    rule_c_fail=1
  fi
  if nm -g "$f" 2>/dev/null | grep -i -q "x265"; then
    echo "  $base exports/imports x265 symbol in Mach-O symbol table"
    rule_c_fail=1
  fi
  # Bare "x265" string allowed only in libvips.42.dylib / libvips-cpp.42.dylib (VipsForeignHeifEncoder enum nick)
  if [[ "$base" != libvips*.dylib ]]; then
    if strings -a "$f" | grep -i -q "x265"; then
      echo "  Non-exempt binary $base contains string 'x265'"
      rule_c_fail=1
    fi
  fi
done

if [[ $rule_c_fail -ne 0 ]]; then
  echo "FAIL (c): Forbidden x265 strings or symbols detected"
  EXIT_CODE=1
else
  echo "PASS (c): Zero x265 symbols or linkage strings across all Mach-O files (enum nick verified absent from all non-libvips files)"
fi

# (d) libde265 present AND linked into libheif via otool -L
rule_d_fail=0
libde265_file="$(find "$VIPS_ROOT/lib" -maxdepth 1 -name "libde265*.dylib" ! -type l | head -n 1 || true)"
libheif_file="$(find "$VIPS_ROOT/lib" -maxdepth 1 -name "libheif*.dylib" ! -type l | head -n 1 || true)"
if [[ -z "$libde265_file" ]]; then
  echo "  Missing regular libde265*.dylib in lib/"
  rule_d_fail=1
fi
if [[ -z "$libheif_file" ]]; then
  echo "  Missing regular libheif*.dylib in lib/"
  rule_d_fail=1
elif ! otool -L "$libheif_file" | grep -q "libde265"; then
  echo "  $(basename "$libheif_file") does NOT dynamically link libde265 in otool -L!"
  rule_d_fail=1
fi

if [[ $rule_d_fail -ne 0 ]]; then
  echo "FAIL (d): libde265 missing or not dynamically linked by libheif"
  EXIT_CODE=1
else
  echo "PASS (d): $(basename "$libde265_file") present and dynamically linked by $(basename "$libheif_file")"
fi

# (e) versions.json completeness, libde265 presence, zero unknown licenses, zero GPL
VERSIONS_FILE="$VIPS_ROOT/versions.json"
if [[ ! -f "$VERSIONS_FILE" ]]; then
  echo "FAIL (e): versions.json missing at $VERSIONS_FILE"
  EXIT_CODE=1
else
  if ! python3 - "$VERSIONS_FILE" "$VIPS_ROOT/lib" <<'PY'
import json
import os
import re
import sys

versions_path, lib_dir = sys.argv[1], sys.argv[2]
data = json.load(open(versions_path, "r", encoding="utf-8"))
comps = data.get("components", [])
if not isinstance(comps, list) or not comps:
    print("  versions.json has empty or invalid 'components' list")
    sys.exit(1)

forbidden_re = re.compile(r"(x265|x264|fftw|imagequant|imagemagick|magick|poppler|rsvg|openslide)", re.IGNORECASE)
gpl_re = re.compile(r"(?<![L])GPL", re.IGNORECASE)

found_libde265 = False
documented_dylibs = set()
lgpl_components = []
permissive_components = []
errors = []

for c in comps:
    name = c.get("name", "")
    lic = c.get("license", "")
    dylibs = c.get("dylibs", [])
    if name == "libde265":
        found_libde265 = True
    if forbidden_re.search(name):
        errors.append(f"Disallowed component in versions.json: {name}")
    if not lic or "unknown" in lic.lower():
        errors.append(f"Component '{name}' has missing or unknown license: {lic!r}")
    elif gpl_re.search(lic):
        errors.append(f"Component '{name}' has disallowed GPL license: {lic!r}")
    elif "LGPL" in lic.upper():
        lgpl_components.append(f"{name} ({lic})")
    else:
        permissive_components.append(f"{name} ({lic})")
    for d in dylibs:
        documented_dylibs.add(d)

disk_dylibs = {
    f for f in os.listdir(lib_dir)
    if f.endswith(".dylib") and not os.path.islink(os.path.join(lib_dir, f))
}
if not found_libde265:
    errors.append("versions.json does not name component 'libde265'")
if disk_dylibs != documented_dylibs:
    missing_docs = sorted(disk_dylibs - documented_dylibs)
    missing_disk = sorted(documented_dylibs - disk_dylibs)
    errors.append(f"Dylib inventory mismatch: missing from versions.json={missing_docs}, missing on disk={missing_disk}")

if errors:
    for e in errors:
        print("  " + e)
    sys.exit(1)

print(f"  Verified {len(comps)} components covering {len(disk_dylibs)} regular dylibs.")
print(f"  LGPL components: {', '.join(lgpl_components)}")
print(f"  Permissive components: {', '.join(permissive_components)}")
print("  GPL status: NONE (0 GPL libraries bundled)")
PY
  then
    echo "FAIL (e): versions.json audit failed"
    EXIT_CODE=1
  else
    echo "PASS (e): versions.json valid, libde265 present, all dylibs accounted for, zero unknown/GPL licenses"
  fi
fi

if [[ $EXIT_CODE -eq 0 ]]; then
  echo "OVERALL: AUDIT PASSED"
else
  echo "OVERALL: AUDIT FAILED"
fi

exit $EXIT_CODE
