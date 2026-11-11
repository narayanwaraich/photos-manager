#!/usr/bin/env bash
# test-audit.sh: Regression suite for deps/macos-vips/audit-dylibs.sh.
# Tests:
#   (i)   known-good synthetic Mach-O bundle (expected exit 0)
#   (ii)  three known-bad synthetic bundles (bad-x265, bad-fftw, bad-homebrew-path; expected exit 1)
#   (iii) large-output pipe/SIGPIPE regressions:
#         - fake `strings` emitting x265_encoder_open on line 1 + >5 MB filler (expected FAIL (c), exit 1)
#         - fake `nm` emitting _x265_foo on line 1 + 200,000 lines (expected FAIL (c), exit 1)
# Every case runs twice: with default SIGPIPE and with SIGPIPE ignored (trap "" PIPE),
# matching the signal disposition of GitHub Actions runners.
#
# Usage: ./deps/macos-vips/test-audit.sh [path/to/audit-dylibs.sh]

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
AUDIT_SCRIPT="${1:-${SCRIPT_DIR}/audit-dylibs.sh}"

if [[ ! -x "$AUDIT_SCRIPT" ]]; then
  echo "ERROR: Audit script not executable or missing: $AUDIT_SCRIPT" >&2
  exit 2
fi

for cmd in cc install_name_tool zip python3 awk; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "ERROR: Missing required tool '$cmd' on PATH." >&2
    exit 2
  fi
done

WORK_TMP="$(mktemp -d "${TMPDIR:-/tmp}/vips-audit-test.XXXXXX")"
# shellcheck disable=SC2329
cleanup() {
  rm -rf "$WORK_TMP"
}
trap cleanup EXIT

BUNDLE_NAME="vips-macos-arm64-8.18.6-hevc"
GOOD_DIR="${WORK_TMP}/good/${BUNDLE_NAME}"
mkdir -p "${GOOD_DIR}/bin" "${GOOD_DIR}/lib"

cat > "${WORK_TMP}/de265.c" <<'EOF'
int de265_decode_stub(void) { return 42; }
EOF

cat > "${WORK_TMP}/heif.c" <<'EOF'
extern int de265_decode_stub(void);
int heif_decode_stub(void) { return de265_decode_stub(); }
EOF

cat > "${WORK_TMP}/heif_x265.c" <<'EOF'
extern int de265_decode_stub(void);
void x265_encoder_open(void) {}
int heif_decode_stub(void) {
  x265_encoder_open();
  return de265_decode_stub();
}
EOF

cat > "${WORK_TMP}/vips.c" <<'EOF'
extern int heif_decode_stub(void);
/* Enum nick matching VipsForeignHeifEncoder in vips foreign.h */
const char *vips_heif_encoder_nick = "x265";
int vips_init_stub(void) {
  return vips_heif_encoder_nick[0] + heif_decode_stub();
}
EOF

cat > "${WORK_TMP}/fftw.c" <<'EOF'
int fftw_plan_stub(void) { return 7; }
EOF

cat > "${WORK_TMP}/vips_cli.c" <<'EOF'
extern int vips_init_stub(void);
int main(void) { return vips_init_stub() == 0 ? 1 : 0; }
EOF

cc -dynamiclib -install_name "@rpath/libde265.0.dylib" \
  "${WORK_TMP}/de265.c" -o "${GOOD_DIR}/lib/libde265.0.dylib"

cc -dynamiclib -install_name "@rpath/libheif.1.dylib" \
  "${WORK_TMP}/heif.c" -L"${GOOD_DIR}/lib" -lde265.0 \
  -o "${GOOD_DIR}/lib/libheif.1.dylib"

cc -dynamiclib -install_name "@rpath/libvips.42.dylib" \
  "${WORK_TMP}/vips.c" -L"${GOOD_DIR}/lib" -lheif.1 \
  -o "${GOOD_DIR}/lib/libvips.42.dylib"

cc "${WORK_TMP}/vips_cli.c" -L"${GOOD_DIR}/lib" -lvips.42 \
  -Wl,-rpath,@executable_path/../lib \
  -o "${GOOD_DIR}/bin/vips"

cat > "${GOOD_DIR}/versions.json" <<'JSON'
{
  "target": "macos-arm64",
  "components": [
    { "name": "libde265", "version": "1.1.1", "license": "LGPL-3.0-or-later", "dylibs": ["libde265.0.dylib"] },
    { "name": "libheif", "version": "1.23.2", "license": "LGPL-3.0-or-later", "dylibs": ["libheif.1.dylib"] },
    { "name": "vips", "version": "8.18.6", "license": "LGPL-2.1-or-later", "dylibs": ["libvips.42.dylib"] }
  ]
}
JSON

(
  cd "${WORK_TMP}/good"
  zip -r -q "${WORK_TMP}/good.zip" "${BUNDLE_NAME}"
)

# Known bad 1: libheif contains/exports x265_encoder_open
mkdir -p "${WORK_TMP}/bad-x265"
cp -R "${GOOD_DIR}" "${WORK_TMP}/bad-x265/${BUNDLE_NAME}"
cc -dynamiclib -install_name "@rpath/libheif.1.dylib" \
  "${WORK_TMP}/heif_x265.c" -L"${WORK_TMP}/bad-x265/${BUNDLE_NAME}/lib" -lde265.0 \
  -o "${WORK_TMP}/bad-x265/${BUNDLE_NAME}/lib/libheif.1.dylib"
(
  cd "${WORK_TMP}/bad-x265"
  zip -r -q "${WORK_TMP}/bad-x265.zip" "${BUNDLE_NAME}"
)

# Known bad 2: disallowed fftw filename + GPL versions.json entry
mkdir -p "${WORK_TMP}/bad-fftw"
cp -R "${GOOD_DIR}" "${WORK_TMP}/bad-fftw/${BUNDLE_NAME}"
cc -dynamiclib -install_name "@rpath/libfftw3.3.dylib" \
  "${WORK_TMP}/fftw.c" -o "${WORK_TMP}/bad-fftw/${BUNDLE_NAME}/lib/libfftw3.3.dylib"
cat > "${WORK_TMP}/bad-fftw/${BUNDLE_NAME}/versions.json" <<'JSON'
{
  "target": "macos-arm64",
  "components": [
    { "name": "libde265", "version": "1.1.1", "license": "LGPL-3.0-or-later", "dylibs": ["libde265.0.dylib"] },
    { "name": "libheif", "version": "1.23.2", "license": "LGPL-3.0-or-later", "dylibs": ["libheif.1.dylib"] },
    { "name": "vips", "version": "8.18.6", "license": "LGPL-2.1-or-later", "dylibs": ["libvips.42.dylib"] },
    { "name": "fftw", "version": "3.3.11", "license": "GPL-2.0-or-later", "dylibs": ["libfftw3.3.dylib"] }
  ]
}
JSON
(
  cd "${WORK_TMP}/bad-fftw"
  zip -r -q "${WORK_TMP}/bad-fftw.zip" "${BUNDLE_NAME}"
)

# Known bad 3: non-relocatable /opt/homebrew path in libheif dependency
mkdir -p "${WORK_TMP}/bad-homebrew"
cp -R "${GOOD_DIR}" "${WORK_TMP}/bad-homebrew/${BUNDLE_NAME}"
install_name_tool -change "@rpath/libde265.0.dylib" "/opt/homebrew/lib/libde265.0.dylib" \
  "${WORK_TMP}/bad-homebrew/${BUNDLE_NAME}/lib/libheif.1.dylib"
(
  cd "${WORK_TMP}/bad-homebrew"
  zip -r -q "${WORK_TMP}/bad-homebrew-path.zip" "${BUNDLE_NAME}"
)

# Fake tool shims for case (iii)
mkdir -p "${WORK_TMP}/shim-strings" "${WORK_TMP}/shim-nm"

cat > "${WORK_TMP}/shim-strings/strings" <<'EOF'
#!/usr/bin/env bash
# Print forbidden symbol on line 1, then >5 MB of filler lines
exec awk 'BEGIN {
  print "x265_encoder_open";
  for (i = 0; i < 750000; i++) {
    print "filler";
  }
}'
EOF
chmod +x "${WORK_TMP}/shim-strings/strings"

cat > "${WORK_TMP}/shim-nm/nm" <<'EOF'
#!/usr/bin/env bash
# Print forbidden symbol on line 1, then 200,000 lines
exec awk 'BEGIN {
  print "_x265_foo";
  for (i = 0; i < 200000; i++) {
    printf "0000000000000000 T _dummy_symbol_%d\n", i;
  }
}'
EOF
chmod +x "${WORK_TMP}/shim-nm/nm"

FAILURES=0

run_check() {
  local label="$1"
  local zip_path="$2"
  local expected_rc="$3"
  local expected_substr="$4"
  local path_prefix="$5"
  local mode rc out effective_path

  for mode in "normal" "sigpipe-ignored"; do
    echo "--- [${mode}] ${label} ---"
    if [[ -n "$path_prefix" ]]; then
      effective_path="${path_prefix}:${PATH}"
    else
      effective_path="$PATH"
    fi

    set +e
    if [[ "$mode" == "normal" ]]; then
      out="$(PATH="$effective_path" "$AUDIT_SCRIPT" "$zip_path" 2>&1)"
      rc=$?
    else
      out="$(PATH="$effective_path" bash -c 'trap "" PIPE; exec "$1" "$2"' _ "$AUDIT_SCRIPT" "$zip_path" 2>&1)"
      rc=$?
    fi
    set -e

    printf '%s\n' "$out"
    echo "EXIT_CODE=${rc} (expected ${expected_rc})"

    if [[ "$rc" -ne "$expected_rc" ]]; then
      echo "TEST ASSERTION FAILED [${mode}] ${label}: got exit ${rc}, expected ${expected_rc}" >&2
      FAILURES=$((FAILURES + 1))
    fi
    if [[ -n "$expected_substr" ]] && [[ "$out" != *"$expected_substr"* ]]; then
      echo "TEST ASSERTION FAILED [${mode}] ${label}: missing expected output substring '${expected_substr}'" >&2
      FAILURES=$((FAILURES + 1))
    fi
  done
}

echo "=== Running audit-dylibs.sh regression suite against: ${AUDIT_SCRIPT} ==="
run_check "(i) KNOWN GOOD (good.zip)" "${WORK_TMP}/good.zip" 0 "OVERALL: AUDIT PASSED" ""
run_check "(ii-1) KNOWN BAD (bad-x265.zip)" "${WORK_TMP}/bad-x265.zip" 1 "FAIL (c)" ""
run_check "(ii-2) KNOWN BAD (bad-fftw.zip)" "${WORK_TMP}/bad-fftw.zip" 1 "FAIL (a)" ""
run_check "(ii-3) KNOWN BAD (bad-homebrew-path.zip)" "${WORK_TMP}/bad-homebrew-path.zip" 1 "FAIL (b)" ""
run_check "(iii-a) FAKE strings (>5MB after x265_encoder_open on line 1)" "${WORK_TMP}/good.zip" 1 "FAIL (c)" "${WORK_TMP}/shim-strings"
run_check "(iii-b) FAKE nm (200k lines after _x265_foo on line 1)" "${WORK_TMP}/good.zip" 1 "FAIL (c)" "${WORK_TMP}/shim-nm"

echo "=== Summary: ${FAILURES} assertion failure(s) ==="
if [[ "$FAILURES" -ne 0 ]]; then
  exit 1
fi
exit 0
