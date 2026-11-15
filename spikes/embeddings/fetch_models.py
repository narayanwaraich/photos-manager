#!/usr/bin/env python3
"""
fetch_models.py

Downloads the pinned EmbeddingGemma 2 Q8 ONNX files, tokenizer, and official
ONNX Runtime v1.31.1 dynamic library using only the Python standard library.
Every download is verified against its pinned 64-character SHA-256 digest and
fails closed on any size or checksum mismatch.

Usage:
  python spikes/embeddings/fetch_models.py
  python spikes/embeddings/fetch_models.py --models-dir spikes/embeddings/models/onnx
  python spikes/embeddings/fetch_models.py --test-bad-checksum
"""

from __future__ import annotations

import argparse
import hashlib
import os
import platform
import shutil
import sys
import tarfile
import urllib.request
import zipfile
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class ArtifactSpec:
    name: str
    url: str
    rel_dest: str
    expected_bytes: int
    expected_sha256: str


HF_ONNX_REV = "daa72c51243991dfcaf9f9137d2c573d8f7790c0"
HF_TOK_REV = "f9972ac22e696a105c482202646979e33e4b5a1e"

MODEL_ARTIFACTS: list[ArtifactSpec] = [
    ArtifactSpec(
        name="tokenizer.json",
        url=f"https://huggingface.co/google/embeddinggemma-2/resolve/{HF_TOK_REV}/tokenizer.json?download=true",
        rel_dest="tokenizer.json",
        expected_bytes=32_170_510,
        expected_sha256="4d777ef5bdc1aa36227abdfb77c3e49e7b9c892d16e1b6bda41c393504828be4",
    ),
    ArtifactSpec(
        name="model_quantized.onnx",
        url=f"https://huggingface.co/onnx-community/embeddinggemma-2-ONNX/resolve/{HF_ONNX_REV}/onnx/model_quantized.onnx?download=true",
        rel_dest="onnx/model_quantized.onnx",
        expected_bytes=495_165,
        expected_sha256="d06edd601f851c633a2519304cbeb8dc6170d7ceb61b436625c17fb9b6e74953",
    ),
    ArtifactSpec(
        name="model_quantized.onnx_data",
        url=f"https://huggingface.co/onnx-community/embeddinggemma-2-ONNX/resolve/{HF_ONNX_REV}/onnx/model_quantized.onnx_data?download=true",
        rel_dest="onnx/model_quantized.onnx_data",
        expected_bytes=313_724_928,
        expected_sha256="278a7ff1248c3618e4bd11a607fc54f7bdc7778854230f3956d3f86bd9db4f3b",
    ),
    ArtifactSpec(
        name="vision_encoder_quantized.onnx",
        url=f"https://huggingface.co/onnx-community/embeddinggemma-2-ONNX/resolve/{HF_ONNX_REV}/onnx/vision_encoder_quantized.onnx?download=true",
        rel_dest="onnx/vision_encoder_quantized.onnx",
        expected_bytes=162_495,
        expected_sha256="bb0de2df53a2448a32dc7908a187c168c8afd514d4d6f674f7f46024875fa4e3",
    ),
    ArtifactSpec(
        name="vision_encoder_quantized.onnx_data",
        url=f"https://huggingface.co/onnx-community/embeddinggemma-2-ONNX/resolve/{HF_ONNX_REV}/onnx/vision_encoder_quantized.onnx_data?download=true",
        rel_dest="onnx/vision_encoder_quantized.onnx_data",
        expected_bytes=195_228_672,
        expected_sha256="3dabd69c0a36e9a8771ad82030dde74daa5a0e02b7047a5d3f3382b1137bab89",
    ),
]

ORT_ARTIFACTS: dict[str, ArtifactSpec] = {
    "darwin-arm64": ArtifactSpec(
        name="onnxruntime-osx-arm64-1.31.1.tgz",
        url="https://github.com/microsoft/onnxruntime/releases/download/v1.31.1/onnxruntime-osx-arm64-1.31.1.tgz",
        rel_dest="onnxruntime-osx-arm64-1.31.1.tgz",
        expected_bytes=36_936_435,
        expected_sha256="2e1344a1926ee50eb25b38153cb6992c3567515aef0e677a1a520a1798896d6f",
    ),
    "win-x64": ArtifactSpec(
        name="onnxruntime-win-x64-1.31.1.zip",
        url="https://github.com/microsoft/onnxruntime/releases/download/v1.31.1/onnxruntime-win-x64-1.31.1.zip",
        rel_dest="onnxruntime-win-x64-1.31.1.zip",
        expected_bytes=82_929_944,
        expected_sha256="5869e26bb25314375f2d5ef1877e3ea06b1543d6c710e6e4a1ec2f7177ab4b2c",
    ),
}


def sha256_file(path: Path) -> tuple[str, int]:
    h = hashlib.sha256()
    total = 0
    with path.open("rb") as f:
        while True:
            chunk = f.read(1024 * 1024)
            if not chunk:
                break
            h.update(chunk)
            total += len(chunk)
    return h.hexdigest(), total


def detect_platform() -> str:
    sys_name = platform.system().lower()
    arch = platform.machine().lower()
    if sys_name == "darwin" and arch in ("arm64", "aarch64"):
        return "darwin-arm64"
    if sys_name == "windows" and arch in ("amd64", "x86_64"):
        return "win-x64"
    raise RuntimeError(f"Unsupported OS/architecture for pinned ONNX Runtime: {sys_name}/{arch}")


def verify_and_fetch(spec: ArtifactSpec, dest: Path, force: bool = False) -> Path:
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.exists() and not force:
        digest, size = sha256_file(dest)
        if size == spec.expected_bytes and digest == spec.expected_sha256:
            print(f"[ok]   {spec.name} ({size} B, sha256 verified)")
            return dest
        print(
            f"[warn] existing {dest} failed verification "
            f"(size={size} vs {spec.expected_bytes}, sha256={digest} vs {spec.expected_sha256}); re-downloading"
        )

    tmp_path = dest.with_name(dest.name + ".part")
    if tmp_path.exists():
        tmp_path.unlink()

    print(f"[get]  {spec.name} <- {spec.url}")
    req = urllib.request.Request(spec.url, headers={"User-Agent": "photos-manager-fetch/1.0"})
    h = hashlib.sha256()
    total = 0
    try:
        with urllib.request.urlopen(req, timeout=300) as resp, tmp_path.open("wb") as out:
            while True:
                chunk = resp.read(1024 * 1024)
                if not chunk:
                    break
                out.write(chunk)
                h.update(chunk)
                total += len(chunk)
    except Exception:
        if tmp_path.exists():
            tmp_path.unlink()
        raise

    digest = h.hexdigest()
    if total != spec.expected_bytes or digest != spec.expected_sha256:
        tmp_path.unlink(missing_ok=True)
        raise RuntimeError(
            f"CHECKSUM/SIZE MISMATCH for {spec.name}: "
            f"got {total} B / sha256={digest}, expected {spec.expected_bytes} B / sha256={spec.expected_sha256}"
        )

    os.replace(tmp_path, dest)
    print(f"[ok]   {spec.name} ({total} B, sha256={digest})")
    return dest


def extract_ort(archive_path: Path, ort_dir: Path, plat: str) -> Path:
    if plat == "darwin-arm64":
        expected_rel = Path("onnxruntime-osx-arm64-1.31.1/lib/libonnxruntime.1.31.1.dylib")
        target_lib = ort_dir / expected_rel
        if not target_lib.exists():
            with tarfile.open(archive_path, "r:gz") as tf:
                # Safe extraction: reject path traversal or absolute paths
                for member in tf.getmembers():
                    member_path = Path(member.name)
                    if member_path.is_absolute() or ".." in member_path.parts:
                        raise RuntimeError(f"Unsafe tar member path: {member.name}")
                tf.extractall(ort_dir)
        if not target_lib.exists():
            raise RuntimeError(f"Expected library missing after extract: {target_lib}")
        return target_lib

    if plat == "win-x64":
        expected_rel = Path("onnxruntime-win-x64-1.31.1/lib/onnxruntime.dll")
        target_lib = ort_dir / expected_rel
        if not target_lib.exists():
            with zipfile.ZipFile(archive_path, "r") as zf:
                for name in zf.namelist():
                    p = Path(name)
                    if p.is_absolute() or ".." in p.parts:
                        raise RuntimeError(f"Unsafe zip member path: {name}")
                zf.extractall(ort_dir)
        if not target_lib.exists():
            # Fallback check in case zip layout differs
            candidates = list(ort_dir.rglob("onnxruntime.dll"))
            if not candidates:
                raise RuntimeError(f"onnxruntime.dll missing after extract under {ort_dir}")
            target_lib = candidates[0]
        return target_lib

    raise RuntimeError(f"Unsupported platform {plat}")


def main() -> int:
    base_dir = Path(__file__).resolve().parent
    parser = argparse.ArgumentParser(description="Fetch pinned Q8 ONNX models, tokenizer, and ORT v1.31.1.")
    parser.add_argument(
        "--models-dir",
        type=Path,
        default=base_dir / "models" / "onnx",
        help="Destination directory for tokenizer.json and onnx/*.onnx[_data]",
    )
    parser.add_argument(
        "--ort-dir",
        type=Path,
        default=base_dir / "models" / "ort-runtime",
        help="Destination directory for pinned ONNX Runtime dynamic library",
    )
    parser.add_argument(
        "--platform",
        choices=["auto", "darwin-arm64", "win-x64", "skip"],
        default="auto",
        help="ONNX Runtime target platform to download/extract (or 'skip')",
    )
    parser.add_argument(
        "--only",
        choices=["all", "models", "ort"],
        default="all",
        help="Fetch all artifacts, models+tokenizer only, or ONNX Runtime only",
    )
    parser.add_argument(
        "--force",
        action="store_true",
        help="Re-download even if existing files match SHA-256",
    )
    parser.add_argument(
        "--test-bad-checksum",
        action="store_true",
        help="Deliberately verify with a wrong SHA-256 on vision_encoder_quantized.onnx and assert non-zero failure",
    )
    args = parser.parse_args()

    if args.test_bad_checksum:
        spec = MODEL_ARTIFACTS[3]  # vision_encoder_quantized.onnx (162 KB)
        tampered = ArtifactSpec(
            name=spec.name + " (tampered-checksum test)",
            url=spec.url,
            rel_dest="tampered_test.onnx",
            expected_bytes=spec.expected_bytes,
            expected_sha256="0" * 64,
        )
        tmp_dir = args.models_dir / ".checksum-test"
        try:
            verify_and_fetch(tampered, tmp_dir / tampered.rel_dest, force=True)
        except RuntimeError as exc:
            print(f"[pass] Negative checksum gate triggered as expected:\n       {exc}")
            shutil.rmtree(tmp_dir, ignore_errors=True)
            return 1
        print("[fail] Negative checksum test unexpectedly succeeded!", file=sys.stderr)
        shutil.rmtree(tmp_dir, ignore_errors=True)
        return 2

    if args.only in ("all", "models"):
        for spec in MODEL_ARTIFACTS:
            verify_and_fetch(spec, args.models_dir / spec.rel_dest, force=args.force)

    if args.only in ("all", "ort") and args.platform != "skip":
        plat = detect_platform() if args.platform == "auto" else args.platform
        ort_spec = ORT_ARTIFACTS[plat]
        archive_path = verify_and_fetch(ort_spec, args.ort_dir / ort_spec.rel_dest, force=args.force)
        lib_path = extract_ort(archive_path, args.ort_dir, plat)
        print(f"[ort]  Ready: {lib_path}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
