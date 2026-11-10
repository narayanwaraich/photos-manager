#!/usr/bin/env bash
# build.sh: Reproducibly build and package a relocatable macOS arm64 libvips 8.18.6
# distribution with HEVC decode (libde265 1.1.1) and AV1 decode (libaom 3.14.1),
# excluding x265, fftw, imagequant, ImageMagick, poppler, librsvg, and modules.
#
# Usage:
#   ./deps/macos-vips/build.sh
#
# Optional environment overrides:
#   VIPS_OUT_DIR        Directory where vips-macos-arm64-8.18.6-hevc.zip is written (default: $PWD)
#   VIPS_SOURCES_DIR    Directory to cache downloaded source tarballs
#   VIPS_BUILD_WORKDIR  Explicit temporary build directory (default: mktemp -d)
#   VIPS_KEEP_WORKDIR   Set to 1 to keep temporary build/staging directory on exit
#   MACOSX_DEPLOYMENT_TARGET  Deployment target (default: major OS version e.g. 15.0)

set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]] || [[ "$(uname -m)" != "arm64" ]]; then
  echo "ERROR: build.sh must run on macOS arm64 (got $(uname -s)/$(uname -m))." >&2
  exit 1
fi

for cmd in cmake meson ninja pkg-config brew curl shasum tar unzip zip otool install_name_tool codesign python3 sw_vers; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "ERROR: Missing required tool '$cmd' on PATH." >&2
    exit 1
  fi
done

VIPS_VER="8.18.6"
VIPS_URL="https://github.com/libvips/libvips/releases/download/v8.18.6/vips-8.18.6.tar.xz"
VIPS_SHA256="3c41e1d5458081bfa4a5bc54e116c46259c75c6760a18027764555632b9dda3e"

LIBHEIF_VER="1.23.2"
LIBHEIF_URL="https://github.com/strukturag/libheif/releases/download/v1.23.2/libheif-1.23.2.tar.gz"
LIBHEIF_SHA256="8bd5d41d19dc84536d118b04774709f244df6104ef66d623dad5fa4650143405"

LIBDE265_VER="1.1.1"
LIBDE265_URL="https://github.com/strukturag/libde265/releases/download/v1.1.1/libde265-1.1.1.tar.gz"
LIBDE265_SHA256="fd48a927e94ed74fc7ce8829d222b9d8599fcbfe8b6448ba66705babc56ab219"

LIBAOM_VER="3.14.1"
LIBAOM_URL="https://storage.googleapis.com/aom-releases/libaom-3.14.1.tar.gz"
LIBAOM_SHA256="44bf90dbd23e734d50e70a8c41c285193922938bd0d3bc2ee56764d181d55ef5"

OUT_DIR="${VIPS_OUT_DIR:-$PWD}"
ARCHIVE_NAME="vips-macos-arm64-8.18.6-hevc"
OUT_ZIP="${OUT_DIR}/${ARCHIVE_NAME}.zip"

WORK_DIR="${VIPS_BUILD_WORKDIR:-$(mktemp -d "${TMPDIR:-/tmp}/vips-macos-build.XXXXXX")}"
# shellcheck disable=SC2329
cleanup() {
  if [[ "${VIPS_KEEP_WORKDIR:-0}" != "1" ]]; then
    rm -rf "$WORK_DIR"
  else
    echo "Keeping workdir: $WORK_DIR"
  fi
}
trap cleanup EXIT

SOURCES_DIR="${VIPS_SOURCES_DIR:-${WORK_DIR}/sources}"
SRC_DIR="${WORK_DIR}/src"
BUILD_DIR="${WORK_DIR}/build"
STAGE_PREFIX="${WORK_DIR}/stage"
DIST_ROOT="${WORK_DIR}/dist/${ARCHIVE_NAME}"

mkdir -p "$OUT_DIR" "$SOURCES_DIR" "$SRC_DIR" "$BUILD_DIR" "$STAGE_PREFIX/lib/pkgconfig" "$DIST_ROOT"

BREW_PREFIX="$(brew --prefix)"
OS_VERSION="$(sw_vers -productVersion)"
OS_MAJOR="${OS_VERSION%%.*}"
DEPLOY_TARGET="${MACOSX_DEPLOYMENT_TARGET:-${OS_MAJOR}.0}"
export MACOSX_DEPLOYMENT_TARGET="$DEPLOY_TARGET"

if command -v xcodebuild >/dev/null 2>&1; then
  XCODE_INFO="$(xcodebuild -version 2>/dev/null | tr '\n' ' ' | sed 's/ *$//')"
else
  XCODE_INFO="$(pkgutil --pkg-info=com.apple.pkg.CLTools_Executables 2>/dev/null | awk -F': ' '/version:/{print "CLT "$2}' || echo 'CLT-unknown')"
fi

echo "=== macOS Build Environment ==="
echo "OS Version:      $OS_VERSION (Deployment target: $MACOSX_DEPLOYMENT_TARGET)"
echo "Toolchain:       $XCODE_INFO"
echo "Brew prefix:     $BREW_PREFIX"
echo "Work directory:  $WORK_DIR"
echo "Output archive:  $OUT_ZIP"

# Allowlisted Homebrew leaf formulas (no x265, no fftw, no imagequant, no rsvg, no magick, no poppler, no raw)
ALLOWED_LEAVES=(
  glib
  gettext
  pcre2
  libffi
  jpeg-turbo
  libpng
  webp
  libtiff
  little-cms2
  libexif
  highway
  zstd
  xz
)

# Construct isolated PKG_CONFIG_LIBDIR containing only our stage prefix,
# explicit allowlisted Homebrew leaf packages, and Homebrew's macOS SDK shims
# (which provide expat.pc, zlib.pc, libffi.pc pointing to /usr/lib system libs).
# Crucially, /opt/homebrew/lib/pkgconfig is NOT included so unrelated runner
# formulas (fftw, x265, imagequant, librsvg, poppler, etc.) are invisible.
PC_DEFAULT="$(pkg-config --variable=pc_path pkg-config)"
OS_PC_DIR=""
IFS=':' read -r -a pc_parts <<< "$PC_DEFAULT"
for p in "${pc_parts[@]}"; do
  if [[ "$p" == *"/os/mac/pkgconfig/"* ]] && [[ -d "$p" ]]; then
    OS_PC_DIR="$p"
    break
  fi
done
if [[ -z "$OS_PC_DIR" ]] && [[ -d "${BREW_PREFIX}/Library/Homebrew/os/mac/pkgconfig/${OS_MAJOR}" ]]; then
  OS_PC_DIR="${BREW_PREFIX}/Library/Homebrew/os/mac/pkgconfig/${OS_MAJOR}"
fi
if [[ -z "$OS_PC_DIR" ]] || [[ ! -f "$OS_PC_DIR/expat.pc" ]] || [[ ! -f "$OS_PC_DIR/zlib.pc" ]]; then
  echo "ERROR: Could not locate macOS system pkg-config shims (expat.pc/zlib.pc). Checked: '$OS_PC_DIR'" >&2
  exit 1
fi

LEAF_PKG_CONFIG_PATH="${STAGE_PREFIX}/lib/pkgconfig"
LEAF_PATHS_LIST=()
for leaf in "${ALLOWED_LEAVES[@]}"; do
  prefix="$(brew --prefix "$leaf")"
  if [[ ! -d "$prefix" ]]; then
    echo "ERROR: Allowlisted Homebrew formula '$leaf' is not installed at $prefix" >&2
    exit 1
  fi
  LEAF_PATHS_LIST+=("${leaf}=${prefix}")
  # Keep macOS SDK /usr/lib shims for libffi ahead of Homebrew keg unless GLib actually linked Homebrew's libffi
  if [[ "$leaf" != "libffi" ]] && [[ -d "${prefix}/lib/pkgconfig" ]]; then
    LEAF_PKG_CONFIG_PATH="${LEAF_PKG_CONFIG_PATH}:${prefix}/lib/pkgconfig"
  fi
done
LEAF_PKG_CONFIG_PATH="${LEAF_PKG_CONFIG_PATH}:${OS_PC_DIR}"

unset PKG_CONFIG_PATH
export PKG_CONFIG_LIBDIR="$LEAF_PKG_CONFIG_PATH"
echo "PKG_CONFIG_LIBDIR=$PKG_CONFIG_LIBDIR"

# Helper to download and strictly verify tarball SHA-256
fetch_and_verify() {
  local name="$1"
  local url="$2"
  local expected_sha="$3"
  local file="${SOURCES_DIR}/${name}"

  if [[ -f "$file" ]]; then
    if echo "${expected_sha}  ${file}" | shasum -a 256 -c - >/dev/null 2>&1; then
      echo "✓ Using cached verified source: $name"
      return 0
    fi
    echo "Cached $name failed checksum; re-downloading..."
    rm -f "$file"
  fi

  echo "Downloading $url ..."
  curl -fL --retry 5 --retry-all-errors --retry-delay 10 --connect-timeout 30 -o "$file" "$url"
  echo "${expected_sha}  ${file}" | shasum -a 256 -c -
}

fetch_and_verify "libde265-${LIBDE265_VER}.tar.gz" "$LIBDE265_URL" "$LIBDE265_SHA256"
fetch_and_verify "libaom-${LIBAOM_VER}.tar.gz" "$LIBAOM_URL" "$LIBAOM_SHA256"
fetch_and_verify "libheif-${LIBHEIF_VER}.tar.gz" "$LIBHEIF_URL" "$LIBHEIF_SHA256"
fetch_and_verify "vips-${VIPS_VER}.tar.xz" "$VIPS_URL" "$VIPS_SHA256"

tar -xzf "${SOURCES_DIR}/libde265-${LIBDE265_VER}.tar.gz" -C "$SRC_DIR"
tar -xzf "${SOURCES_DIR}/libaom-${LIBAOM_VER}.tar.gz" -C "$SRC_DIR"
tar -xzf "${SOURCES_DIR}/libheif-${LIBHEIF_VER}.tar.gz" -C "$SRC_DIR"
tar -xJf "${SOURCES_DIR}/vips-${VIPS_VER}.tar.xz" -C "$SRC_DIR"

echo "=== 1. Building libde265 ${LIBDE265_VER} ==="
cmake -S "${SRC_DIR}/libde265-${LIBDE265_VER}" -B "${BUILD_DIR}/libde265" -G Ninja \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_INSTALL_PREFIX="$STAGE_PREFIX" \
  -DCMAKE_INSTALL_LIBDIR=lib \
  -DCMAKE_OSX_DEPLOYMENT_TARGET="$DEPLOY_TARGET" \
  -DCMAKE_INSTALL_NAME_DIR="@rpath" \
  -DCMAKE_SHARED_LINKER_FLAGS="-Wl,-headerpad_max_install_names" \
  -DBUILD_SHARED_LIBS=ON \
  -DBUILD_FRAMEWORK=OFF \
  -DENABLE_SDL=OFF \
  -DENABLE_SIMD=ON \
  -DENABLE_DECODER=OFF \
  -DENABLE_ENCODER=OFF \
  -DENABLE_SHERLOCK265=OFF \
  -DENABLE_INTERNAL_DEVELOPMENT_TOOLS=OFF \
  -DWITH_FUZZERS=OFF
cmake --build "${BUILD_DIR}/libde265" --parallel
cmake --install "${BUILD_DIR}/libde265"

echo "=== 2. Building libaom ${LIBAOM_VER} (decode-only) ==="
cmake -S "${SRC_DIR}/libaom-${LIBAOM_VER}" -B "${BUILD_DIR}/libaom" -G Ninja \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_INSTALL_PREFIX="$STAGE_PREFIX" \
  -DCMAKE_INSTALL_LIBDIR=lib \
  -DCMAKE_OSX_DEPLOYMENT_TARGET="$DEPLOY_TARGET" \
  -DCMAKE_INSTALL_NAME_DIR="@rpath" \
  -DCMAKE_SHARED_LINKER_FLAGS="-Wl,-headerpad_max_install_names" \
  -DBUILD_SHARED_LIBS=ON \
  -DCONFIG_AV1_DECODER=1 \
  -DCONFIG_AV1_ENCODER=0 \
  -DCONFIG_AV1_HIGHBITDEPTH=1 \
  -DCONFIG_WEBM_IO=0 \
  -DCONFIG_HIGHWAY=0 \
  -DENABLE_DOCS=OFF \
  -DENABLE_EXAMPLES=OFF \
  -DENABLE_TESTDATA=OFF \
  -DENABLE_TESTS=OFF \
  -DENABLE_TOOLS=OFF
cmake --build "${BUILD_DIR}/libaom" --parallel
cmake --install "${BUILD_DIR}/libaom"

echo "=== 3. Building libheif ${LIBHEIF_VER} (libde265 + aom decoder, NO x265) ==="
WEBP_PREFIX="$(brew --prefix webp)"
cmake -S "${SRC_DIR}/libheif-${LIBHEIF_VER}" -B "${BUILD_DIR}/libheif" -G Ninja \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_INSTALL_PREFIX="$STAGE_PREFIX" \
  -DCMAKE_INSTALL_LIBDIR=lib \
  -DCMAKE_OSX_DEPLOYMENT_TARGET="$DEPLOY_TARGET" \
  -DCMAKE_INSTALL_NAME_DIR="@rpath" \
  -DCMAKE_INSTALL_RPATH="@loader_path" \
  -DCMAKE_BUILD_WITH_INSTALL_RPATH=OFF \
  -DCMAKE_PREFIX_PATH="${STAGE_PREFIX};${WEBP_PREFIX}" \
  -DCMAKE_SHARED_LINKER_FLAGS="-Wl,-headerpad_max_install_names" \
  -DBUILD_SHARED_LIBS=ON \
  -DBUILD_FRAMEWORK=OFF \
  -DENABLE_PLUGIN_LOADING=OFF \
  -DWITH_LIBDE265=ON \
  -DWITH_LIBDE265_PLUGIN=OFF \
  -DWITH_X265=OFF \
  -DWITH_X264=OFF \
  -DWITH_OpenH264_DECODER=OFF \
  -DWITH_KVAZAAR=OFF \
  -DWITH_UVG266=OFF \
  -DWITH_VVDEC=OFF \
  -DWITH_VVENC=OFF \
  -DWITH_DAV1D=OFF \
  -DWITH_AOM_DECODER=ON \
  -DWITH_AOM_DECODER_PLUGIN=OFF \
  -DWITH_AOM_ENCODER=OFF \
  -DWITH_SvtEnc=OFF \
  -DWITH_RAV1E=OFF \
  -DWITH_JPEG_DECODER=OFF \
  -DWITH_JPEG_ENCODER=OFF \
  -DWITH_OpenJPEG_ENCODER=OFF \
  -DWITH_OpenJPEG_DECODER=OFF \
  -DWITH_FFMPEG_DECODER=OFF \
  -DWITH_OPENJPH_ENCODER=OFF \
  -DWITH_UNCOMPRESSED_CODEC=OFF \
  -DWITH_WEBCODECS=OFF \
  -DWITH_LIBSHARPYUV=ON \
  -DWITH_LIBSHARPYUV_INTERNAL=OFF \
  -DWITH_EXAMPLES=OFF \
  -DWITH_EXAMPLE_HEIF_THUMB=OFF \
  -DWITH_EXAMPLE_HEIF_VIEW=OFF \
  -DWITH_GDK_PIXBUF=OFF \
  -DWITH_REDUCED_VISIBILITY=ON \
  -DWITH_HEADER_COMPRESSION=OFF \
  -DBUILD_DOCUMENTATION=OFF \
  -DBUILD_TESTING=OFF \
  -DWITH_FUZZERS=OFF
cmake --build "${BUILD_DIR}/libheif" --parallel
cmake --install "${BUILD_DIR}/libheif"

echo "--- Verifying libheif linkage before building libvips ---"
otool -L "${STAGE_PREFIX}/lib/libheif.dylib"
if ! otool -L "${STAGE_PREFIX}/lib/libheif.dylib" | grep -q "libde265"; then
  echo "ERROR: libheif.dylib does not dynamically link libde265!" >&2
  exit 1
fi
if ! otool -L "${STAGE_PREFIX}/lib/libheif.dylib" | grep -q "libaom"; then
  echo "ERROR: libheif.dylib does not dynamically link libaom!" >&2
  exit 1
fi
if otool -L "${STAGE_PREFIX}/lib/libheif.dylib" | grep -qi "x265"; then
  echo "ERROR: libheif.dylib unexpectedly links x265!" >&2
  exit 1
fi

echo "=== 4. Building libvips ${VIPS_VER} (modules disabled, built-in heifload) ==="
GETTEXT_PREFIX="$(brew --prefix gettext)"
CFLAGS="-I${GETTEXT_PREFIX}/include" \
CXXFLAGS="-I${GETTEXT_PREFIX}/include" \
CPPFLAGS="-I${GETTEXT_PREFIX}/include" \
LDFLAGS="-L${GETTEXT_PREFIX}/lib -Wl,-headerpad_max_install_names" \
meson setup "${BUILD_DIR}/vips" "${SRC_DIR}/vips-${VIPS_VER}" \
  --prefix="$STAGE_PREFIX" \
  --libdir=lib \
  --buildtype=release \
  -Dmodules=disabled \
  -Dintrospection=disabled \
  -Dvapi=false \
  -Ddocs=false \
  -Dcpp-docs=false \
  -Dexamples=false \
  -Ddeprecated=false \
  -Dcplusplus=false \
  -Dheif=enabled \
  -Dheif-module=disabled \
  -Djpeg=enabled \
  -Dpng=enabled \
  -Dspng=disabled \
  -Dwebp=enabled \
  -Dtiff=enabled \
  -Dlcms=enabled \
  -Dexif=enabled \
  -Dhighway=enabled \
  -Dorc=disabled \
  -Dzlib=enabled \
  -Dnsgif=true \
  -Dcgif=disabled \
  -Dfftw=disabled \
  -Dimagequant=disabled \
  -Dquantizr=disabled \
  -Dmagick=disabled \
  -Dmagick-module=disabled \
  -Drsvg=disabled \
  -Dpoppler=disabled \
  -Dpoppler-module=disabled \
  -Dopenslide=disabled \
  -Dopenslide-module=disabled \
  -Dopenexr=disabled \
  -Dopenjpeg=disabled \
  -Djpeg-xl=disabled \
  -Djpeg-xl-module=disabled \
  -Dnifti=disabled \
  -Dmatio=disabled \
  -Dpdfium=disabled \
  -Dcfitsio=disabled \
  -Darchive=disabled \
  -Dpangocairo=disabled \
  -Dfontconfig=disabled \
  -Duhdr=disabled \
  -Draw=disabled \
  -Dppm=false \
  -Danalyze=false \
  -Dradiance=false

meson compile -C "${BUILD_DIR}/vips"
meson install -C "${BUILD_DIR}/vips"

if [[ -d "${STAGE_PREFIX}/lib/vips-modules-8.18" ]]; then
  echo "ERROR: vips-modules-8.18 directory should not exist with -Dmodules=disabled!" >&2
  exit 1
fi

echo "=== 5. Harvesting closure, relocating Mach-O @rpaths, ad-hoc signing, generating versions.json & LICENSES.md ==="
export STAGE_PREFIX DIST_ROOT BREW_PREFIX OS_VERSION XCODE_INFO DEPLOY_TARGET OUT_ZIP
export VIPS_VER VIPS_URL VIPS_SHA256
export LIBHEIF_VER LIBHEIF_URL LIBHEIF_SHA256
export LIBDE265_VER LIBDE265_URL LIBDE265_SHA256
export LIBAOM_VER LIBAOM_URL LIBAOM_SHA256
export ALLOWED_LEAVES_STR="${ALLOWED_LEAVES[*]}"

python3 - <<'PY'
import datetime
import json
import os
import re
import shutil
import subprocess

stage_prefix = os.path.realpath(os.environ["STAGE_PREFIX"])
dist_root = os.path.realpath(os.environ["DIST_ROOT"])
brew_prefix = os.path.realpath(os.environ["BREW_PREFIX"])
allowed_leaves = os.environ["ALLOWED_LEAVES_STR"].split()

dist_bin = os.path.join(dist_root, "bin")
dist_lib = os.path.join(dist_root, "lib")
dist_inc = os.path.join(dist_root, "include")
dist_pc = os.path.join(dist_lib, "pkgconfig")
dist_share = os.path.join(dist_root, "share")

os.makedirs(dist_bin, exist_ok=True)
os.makedirs(dist_lib, exist_ok=True)
os.makedirs(dist_inc, exist_ok=True)
os.makedirs(dist_pc, exist_ok=True)

leaf_lib_dirs = []
for leaf in allowed_leaves:
    p = os.path.realpath(os.path.join(brew_prefix, "opt", leaf, "lib"))
    if os.path.isdir(p):
        leaf_lib_dirs.append((leaf, p))

cellar_root = os.path.realpath(os.path.join(brew_prefix, "Cellar"))

def run(cmd, check=True):
    return subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, check=check)

def otool_deps(path):
    res = run(["otool", "-L", path])
    lines = res.stdout.splitlines()
    id_res = run(["otool", "-D", path])
    id_lines = [l.strip() for l in id_res.stdout.splitlines()[1:] if l.strip()]
    skip = 2 if id_lines else 1
    deps = []
    for line in lines[skip:]:
        s = line.strip()
        if not s:
            continue
        m = re.match(r"^([^\s]+)\s+\(compatibility", s)
        if m:
            deps.append(m.group(1))
    return deps, (len(id_lines) > 0)

def otool_rpaths(path):
    res = run(["otool", "-l", path])
    lines = res.stdout.splitlines()
    rpaths = []
    i = 0
    while i < len(lines):
        if lines[i].strip() == "cmd LC_RPATH":
            for j in range(i + 1, min(i + 8, len(lines))):
                m = re.match(r"^\s*path\s+(.+?)\s+\(offset", lines[j])
                if m:
                    rpaths.append(m.group(1))
                    break
        i += 1
    return rpaths

def is_system_lib(dep):
    return dep.startswith("/System/") or dep.startswith("/usr/lib/")

def classify_origin(real_path, target_name):
    if real_path.startswith(stage_prefix + os.sep):
        if target_name.startswith("libvips"):
            return ("source", "vips")
        if target_name.startswith("libheif"):
            return ("source", "libheif")
        if target_name.startswith("libde265"):
            return ("source", "libde265")
        if target_name.startswith("libaom"):
            return ("source", "aom")
        return ("source", "stage")
    if real_path.startswith(cellar_root + os.sep):
        rel = real_path[len(cellar_root) + 1:]
        formula = rel.split(os.sep)[0]
        if formula not in allowed_leaves:
            raise RuntimeError(f"Disallowed Homebrew formula '{formula}' pulled in via {real_path}")
        return ("brew", formula)
    raise RuntimeError(f"Unexpected non-system dependency outside stage/allowlisted Cellar: {real_path}")

def resolve_dep(dep, origin_dir):
    base = os.path.basename(dep)
    if dep.startswith("@rpath/") or dep.startswith("@loader_path/"):
        cand_stage = os.path.join(stage_prefix, "lib", base)
        if os.path.exists(cand_stage):
            return os.path.realpath(cand_stage), base
        cand_rel = os.path.join(origin_dir, base)
        if os.path.exists(cand_rel):
            real_c = os.path.realpath(cand_rel)
            classify_origin(real_c, base)
            return real_c, base
        for _, ldir in leaf_lib_dirs:
            cand = os.path.join(ldir, base)
            if os.path.exists(cand):
                real_c = os.path.realpath(cand)
                classify_origin(real_c, base)
                return real_c, base
        raise RuntimeError(f"Unable to resolve relative/rpath dependency {dep} from {origin_dir}")
    if dep.startswith("/"):
        if not os.path.exists(dep):
            raise RuntimeError(f"Absolute dependency does not exist on disk: {dep}")
        real_d = os.path.realpath(dep)
        classify_origin(real_d, base)
        return real_d, base
    raise RuntimeError(f"Unsupported dependency reference format: {dep}")

def find_soname_file(prefix_dir, stem):
    lib_dir = os.path.join(stage_prefix, "lib")
    for entry in sorted(os.listdir(lib_dir)):
        if entry.startswith(stem) and entry.endswith(".dylib"):
            full = os.path.join(lib_dir, entry)
            if not os.path.islink(full):
                id_res = run(["otool", "-D", full])
                id_lines = [l.strip() for l in id_res.stdout.splitlines()[1:] if l.strip()]
                if id_lines:
                    return os.path.realpath(full), os.path.basename(id_lines[0])
                return os.path.realpath(full), entry
    raise RuntimeError(f"Could not find built library matching {stem}*.dylib in {lib_dir}")

queue = []
for stem in ["libvips.", "libheif.", "libde265.", "libaom."]:
    rpath, sname = find_soname_file(stage_prefix, stem)
    queue.append((rpath, sname, False))

for tool in ["vips", "vipsthumbnail", "vipsheader", "vipsedit"]:
    tpath = os.path.join(stage_prefix, "bin", tool)
    if not os.path.isfile(tpath):
        raise RuntimeError(f"Missing expected CLI tool: {tpath}")
    queue.append((os.path.realpath(tpath), tool, True))

seen_targets = {}
target_origins = {}
brew_formulas_used = set()
staged_macho = []

while queue:
    real_src, target_name, is_exe = queue.pop(0)
    key = ("bin" if is_exe else "lib", target_name)
    if key in seen_targets:
        continue
    seen_targets[key] = real_src

    dest_dir = dist_bin if is_exe else dist_lib
    dest_path = os.path.join(dest_dir, target_name)
    shutil.copy2(real_src, dest_path)
    os.chmod(dest_path, 0o755)
    staged_macho.append((dest_path, target_name, is_exe))

    if not is_exe:
        kind, comp = classify_origin(real_src, target_name)
        target_origins[target_name] = (kind, comp)
        if kind == "brew":
            brew_formulas_used.add(comp)

    deps, _ = otool_deps(real_src)
    origin_dir = os.path.dirname(real_src)
    for dep in deps:
        if is_system_lib(dep):
            continue
        dep_real, dep_soname = resolve_dep(dep, origin_dir)
        if ("lib", dep_soname) not in seen_targets:
            queue.append((dep_real, dep_soname, False))

print(f"Closure collected: {len([k for k in seen_targets if k[0] == 'lib'])} dylibs, {len([k for k in seen_targets if k[0] == 'bin'])} executables")

# Relocate all Mach-O files strictly to @rpath / @loader_path / @executable_path/../lib
for dest_path, target_name, is_exe in staged_macho:
    deps, _ = otool_deps(dest_path)
    args = []
    if not is_exe:
        args.extend(["-id", f"@rpath/{target_name}"])
    for dep in deps:
        if is_system_lib(dep):
            continue
        dep_base = os.path.basename(dep)
        new_dep = f"@rpath/{dep_base}"
        if not os.path.exists(os.path.join(dist_lib, dep_base)):
            raise RuntimeError(f"Broken rpath closure: {target_name} needs {dep_base} which is missing from {dist_lib}")
        if dep != new_dep:
            args.extend(["-change", dep, new_dep])
    existing_rpaths = otool_rpaths(dest_path)
    desired_rpath = "@executable_path/../lib" if is_exe else "@loader_path"
    seen_rp = set()
    for rp in existing_rpaths:
        if rp in seen_rp or rp != desired_rpath:
            args.extend(["-delete_rpath", rp])
        seen_rp.add(rp)
    if desired_rpath not in existing_rpaths:
        args.extend(["-add_rpath", desired_rpath])

    if args:
        run(["install_name_tool"] + args + [dest_path])
    run(["codesign", "--force", "-s", "-", dest_path])
    run(["codesign", "--verify", "--strict", dest_path])

# Verify zero non-system absolute paths remain anywhere
for dest_path, target_name, is_exe in staged_macho:
    res = run(["otool", "-L", dest_path])
    for line in res.stdout.splitlines()[1:]:
        m = re.match(r"^(.+?)\s+\(compatibility", line.strip())
        if not m:
            continue
        path_tok = m.group(1)
        if not (path_tok.startswith("/System/") or path_tok.startswith("/usr/lib/") or path_tok.startswith("@rpath/")):
            raise RuntimeError(f"Leak detected after relocation in {dest_path}: {path_tok}")

# Create compile-time unversioned symlinks (not copied by package-bundle.mjs at runtime)
# so `cargo build` (-lvips -lglib-2.0 -lgobject-2.0) links cleanly against $VIPS_LIB_DIR.
all_libs = set(os.listdir(dist_lib))
dev_symlink_prefixes = ["libvips.", "libglib-2.0.", "libgobject-2.0.", "libgio-2.0.", "libgmodule-2.0.", "libheif.", "libde265.", "libaom."]
for fname in sorted(all_libs):
    for pfx in dev_symlink_prefixes:
        if fname.startswith(pfx) and fname.endswith(".dylib"):
            unver = pfx + "dylib"
            if unver != fname and not os.path.exists(os.path.join(dist_lib, unver)):
                os.symlink(fname, os.path.join(dist_lib, unver))

# Copy headers and share/
shutil.copytree(os.path.join(stage_prefix, "include"), dist_inc, dirs_exist_ok=True)
stage_share = os.path.join(stage_prefix, "share")
if os.path.isdir(stage_share):
    shutil.copytree(stage_share, dist_share, dirs_exist_ok=True)

# Copy and make pkg-config files relocatable via ${pcfiledir}
stage_pc = os.path.join(stage_prefix, "lib", "pkgconfig")
raw_stage_prefix = os.environ["STAGE_PREFIX"]
prefix_variants = sorted(
    {
        raw_stage_prefix,
        os.path.normpath(raw_stage_prefix),
        stage_prefix,
        re.sub(r"//+", "/", raw_stage_prefix),
    },
    key=len,
    reverse=True,
)
if os.path.isdir(stage_pc):
    for pc in sorted(os.listdir(stage_pc)):
        if pc.endswith(".pc"):
            content = open(os.path.join(stage_pc, pc), "r", encoding="utf-8").read()
            for pfx in prefix_variants:
                content = content.replace(pfx, "${pcfiledir}/../..")
            open(os.path.join(dist_pc, pc), "w", encoding="utf-8").write(content)

# Build versions.json from actual tarball metadata + brew info --json=v2
brew_meta = {}
if brew_formulas_used:
    b_res = run(["brew", "info", "--json=v2"] + sorted(brew_formulas_used))
    b_json = json.loads(b_res.stdout)
    for f in b_json.get("formulae", []):
        brew_meta[f["name"]] = f

# Group runtime dylibs by component
comp_dylibs = {}
for target_name, (kind, comp) in sorted(target_origins.items()):
    comp_dylibs.setdefault((kind, comp), []).append(target_name)

# Explicit verified runtime-only license mappings where a formula's CLI utilities are GPL
# but its shared library is LGPL or permissive (CLI utilities are NOT bundled):
VERIFIED_RUNTIME_LICENSE_SPLITS = {
    "gettext": {
        "expected_raw": "GPL-3.0-or-later AND LGPL-2.1-or-later",
        "expected_dylibs": {"libintl.8.dylib"},
        "effective": "LGPL-2.1-or-later",
        "note": "Runtime libintl shared library is LGPL-2.1-or-later; GPL-3.0-or-later applies only to gettext CLI build tools (not bundled).",
    },
    "xz": {
        "expected_raw": "0BSD AND GPL-2.0-or-later",
        "expected_dylibs": {"liblzma.5.dylib"},
        "effective": "0BSD",
        "note": "Runtime liblzma shared library is 0BSD public domain; GPL-2.0-or-later applies only to xz CLI utilities/scripts (not bundled).",
    },
    "zstd": {
        "expected_raw": "(BSD-3-Clause OR GPL-2.0-only) AND BSD-2-Clause AND MIT",
        "expected_dylibs": {"libzstd.1.dylib"},
        "effective": "BSD-3-Clause",
        "note": "Dual-licensed (BSD-3-Clause OR GPL-2.0-only); licensee elects BSD-3-Clause alongside BSD-2-Clause AND MIT.",
    },
}

SOURCE_COMPONENTS = {
    "vips": (os.environ["VIPS_VER"], os.environ["VIPS_URL"], os.environ["VIPS_SHA256"], "LGPL-2.1-or-later"),
    "libheif": (os.environ["LIBHEIF_VER"], os.environ["LIBHEIF_URL"], os.environ["LIBHEIF_SHA256"], "LGPL-3.0-or-later"),
    "libde265": (os.environ["LIBDE265_VER"], os.environ["LIBDE265_URL"], os.environ["LIBDE265_SHA256"], "LGPL-3.0-or-later"),
    "aom": (os.environ["LIBAOM_VER"], os.environ["LIBAOM_URL"], os.environ["LIBAOM_SHA256"], "BSD-2-Clause"),
}

components_out = []
for (kind, comp), dylibs in sorted(comp_dylibs.items(), key=lambda item: item[0][1]):
    if kind == "source":
        ver, url, sha, lic = SOURCE_COMPONENTS[comp]
        components_out.append({
            "name": comp,
            "version": ver,
            "license": lic,
            "rawFormulaLicense": lic,
            "source": f"tarball:{url}",
            "sha256": sha,
            "dylibs": sorted(dylibs),
        })
    else:
        f = brew_meta.get(comp, {})
        ver = f.get("versions", {}).get("stable") or "unknown"
        rev = f.get("revision", 0)
        full_ver = f"{ver}_{rev}" if rev else ver
        raw_lic = f.get("license")
        if not raw_lic or not isinstance(raw_lic, str) or not raw_lic.strip():
            eff_lic = "unknown, check upstream"
            raw_str = "unknown, check upstream"
            note = "brew info returned no license field; do not guess."
        else:
            raw_str = raw_lic.strip()
            split_rule = VERIFIED_RUNTIME_LICENSE_SPLITS.get(comp)
            if (
                split_rule is not None
                and raw_str == split_rule["expected_raw"]
                and set(dylibs).issubset(split_rule["expected_dylibs"])
            ):
                eff_lic = split_rule["effective"]
                note = split_rule["note"]
            else:
                eff_lic = raw_str
                note = None
        entry = {
            "name": comp,
            "version": full_ver,
            "license": eff_lic,
            "rawFormulaLicense": raw_str,
            "source": f"homebrew:{comp}",
            "dylibs": sorted(dylibs),
        }
        if note:
            entry["licenseNote"] = note
        components_out.append(entry)

versions_doc = {
    "target": "macos-arm64",
    "archive": os.path.basename(os.environ["OUT_ZIP"]),
    "builtAt": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "runnerOsVersion": os.environ["OS_VERSION"],
    "deploymentTarget": os.environ["DEPLOY_TARGET"],
    "xcode": os.environ["XCODE_INFO"],
    "systemLibrariesUsed": [
        "/System/Library/Frameworks/*",
        "/usr/lib/libSystem.B.dylib",
        "/usr/lib/libc++.1.dylib",
        "/usr/lib/libexpat.1.dylib",
        "/usr/lib/libffi.dylib",
        "/usr/lib/libiconv.2.dylib",
        "/usr/lib/libobjc.A.dylib",
        "/usr/lib/libresolv.9.dylib",
        "/usr/lib/libz.1.dylib",
    ],
    "components": components_out,
}

with open(os.path.join(dist_root, "versions.json"), "w", encoding="utf-8") as fh:
    json.dump(versions_doc, fh, indent=2)
    fh.write("\n")

lgpl_list = [c["name"] for c in components_out if "LGPL" in c["license"]]
perm_list = [c["name"] for c in components_out if "LGPL" not in c["license"]]

lines = [
    "# Bundled Component Licenses (macOS arm64 libvips HEVC-decode build)",
    "",
    f"- **Built at:** {versions_doc['builtAt']}",
    f"- **macOS version:** {versions_doc['runnerOsVersion']} (deployment target {versions_doc['deploymentTarget']})",
    f"- **Toolchain:** {versions_doc['xcode']}",
    f"- **LGPL libraries (dynamically linked, permitted):** {', '.join(lgpl_list)}",
    f"- **Permissive libraries:** {', '.join(perm_list)}",
    "- **GPL libraries bundled:** None (x265, fftw, and imagequant 4.x are excluded from build and linkage).",
    "- **Patent note:** HEVC codec patent licensing is separate from open-source library copyright licenses (D13 / Phase 7).",
    "",
    "| Component | Version | Bundled `.dylib`(s) | License | Source | Notes |",
    "|---|---|---|---|---|---|",
]
for c in components_out:
    dylibs_str = ", ".join(f"`{d}`" for d in c["dylibs"])
    note_str = c.get("licenseNote", "")
    lines.append(f"| `{c['name']}` | `{c['version']}` | {dylibs_str} | `{c['license']}` | `{c['source']}` | {note_str} |")

with open(os.path.join(dist_root, "LICENSES.md"), "w", encoding="utf-8") as fh:
    fh.write("\n".join(lines) + "\n")

print("Generated versions.json and LICENSES.md:")
print(f"  LGPL ({len(lgpl_list)}): {', '.join(lgpl_list)}")
print(f"  Permissive ({len(perm_list)}): {', '.join(perm_list)}")
PY

echo "=== 6. Creating relocatable zip archive ==="
rm -f "$OUT_ZIP"
(
  cd "${WORK_DIR}/dist"
  zip -r -y -q "$OUT_ZIP" "$ARCHIVE_NAME"
)

echo "✓ Built: $OUT_ZIP"
ls -lh "$OUT_ZIP"
shasum -a 256 "$OUT_ZIP"
