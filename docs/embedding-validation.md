# Windows Embedding Parity & Benchmark Validation Guide

**Target Platform:** Windows 10/11 x64 (`x86_64-pc-windows-msvc`)  
**Reference Low-End Machine:** Acer Swift SF314-43, Ryzen 7 5700U, 8 GB RAM, Windows 11  
**Scope:** TASK-0B2 embedding parity and runtime measurement using **only** committed synthetic fixtures (never real user photos).

> [!IMPORTANT]
> **Why This Laptop Run Matters:**
> GitHub Actions `windows-latest` runners measure Xeon/EPYC server CPUs with 16 GB RAM. Only the owner's 8 GB Ryzen 7 5700U laptop proves real consumer indexing throughput, ETA for 10k/100k photos, and Peak Working Set headroom alongside the OS and WebView2.

---

## 1. Prerequisites

1. **Python 3.10+** (standard library only; no `pip install`, no virtualenv required) and **Git**.
2. Create a clean temporary validation directory (e.g., `C:\emb-val\`).
3. Ensure `task/0B2` (or `main` after merge) is checked out locally so the committed synthetic fixtures (`spikes/embeddings/fixtures/`, `spikes/embeddings/parity/`) and `spikes/embeddings/fetch_models.py` are available.
   *(Do **not** copy any personal photos into the validation folder).*

---

## 2. Download the CI Binary Artifact

1. Open the repository **Actions** tab on GitHub and select the green **Embedding Parity Spike** (`spike-embeddings.yml`) run on `task/0B2`.
2. Download the **`embedding-parity-windows-x64`** artifact and extract it into `C:\emb-val\bin\`.
3. Unblock downloaded executable files in PowerShell:
   ```powershell
   Get-ChildItem -Path "C:\emb-val\bin" -Recurse | Unblock-File
   ```
4. *(If building locally instead of downloading the CI artifact)*:
   ```powershell
   $env:CARGO_BUILD_JOBS = "2"
   cargo build --release --manifest-path spikes\embeddings\rust-parity\Cargo.toml
   Copy-Item "spikes\embeddings\rust-parity\target\release\embedding-parity.exe" "C:\emb-val\bin\embedding-parity.exe"
   ```

---

## 3. Fetch & Checksum-Verify Weights and ONNX Runtime `v1.31.1`

From a copy or checkout of the repository containing **only** committed synthetic files (e.g., `C:\emb-val\repo`), run one standard-library command to download and SHA-256-verify `tokenizer.json`, the four Q8 ONNX files, and `onnxruntime-win-x64-1.31.1.zip`:

```powershell
cd C:\emb-val\repo
python spikes\embeddings\fetch_models.py --platform win-x64
```

Verify every line prints `[ok] ... sha256 verified` and finishes with:
`[ort] Ready: ...\spikes\embeddings\models\ort-runtime\onnxruntime-win-x64-1.31.1\...\onnxruntime.dll`.

Record host VC++ runtime state:
```powershell
Test-Path "C:\Windows\System32\vcruntime140.dll"
```

---

## 4. Run Parity Gates & Laptop Benchmarks (Stripped PATH)

Execute the clean-environment run with only OS system directories on `PATH`, saving the full output for the orchestrator:

```powershell
$bin = "C:\emb-val\bin\embedding-parity.exe"
$ortDll = (Get-ChildItem -Path "spikes\embeddings\models\ort-runtime" -Filter "onnxruntime.dll" -Recurse | Select-Object -First 1).FullName
$log = "C:\emb-val\laptop-results.txt"

$origPath = $env:PATH
$env:PATH = "C:\Windows\System32;C:\Windows;C:\Windows\System32\Wbem"

try {
    "=== HARDWARE & OS ===" | Out-File -FilePath $log -Encoding utf8
    Get-CimInstance Win32_Processor | Select-Object Name, NumberOfCores, NumberOfLogicalProcessors | Out-File -FilePath $log -Append -Encoding utf8
    Get-CimInstance Win32_ComputerSystem | Select-Object TotalPhysicalMemory | Out-File -FilePath $log -Append -Encoding utf8

    "=== 1. FULL PARITY COMPARE ===" | Out-File -FilePath $log -Append -Encoding utf8
    & $bin --ort-lib $ortDll --threads 4 compare --preview-test | Tee-Object -FilePath $log -Append
    if ($LASTEXITCODE -ne 0) { throw "compare failed with exit code $LASTEXITCODE" }

    "=== 2. THREAD SWEEP (1, 2, 4, 8) & BUDGETS (70, 140, 280) ===" | Out-File -FilePath $log -Append -Encoding utf8
    & $bin --ort-lib $ortDll bench --mode sweep --sweep-threads 1,2,4,8 --budgets 70,140,280 --batches 1,2,4 --num-images 12 | Tee-Object -FilePath $log -Append

    "=== 3. B70 vs B140 ===" | Out-File -FilePath $log -Append -Encoding utf8
    & $bin --ort-lib $ortDll --threads 4 bench --mode b70-vs-b140 | Tee-Object -FilePath $log -Append

    "=== 4. DIRECTML EP PROBE (optional if supported) ===" | Out-File -FilePath $log -Append -Encoding utf8
    & $bin --ort-lib $ortDll --threads 4 bench --mode ep-compare --vision-ep directml --num-images 8 2>&1 | Tee-Object -FilePath $log -Append

    "=== 5. CLEAN-PROCESS PEAK WORKING SET ===" | Out-File -FilePath $log -Append -Encoding utf8
    foreach ($m in @("single-text", "single-image", "single-drop-vision")) {
        $psi = New-Object System.Diagnostics.ProcessStartInfo
        $psi.FileName = $bin
        $psi.Arguments = "--ort-lib `"$ortDll`" --threads 4 bench --mode $m --budget 140"
        $psi.RedirectStandardOutput = $true
        $psi.RedirectStandardError = $true
        $psi.UseShellExecute = $false
        $p = [System.Diagnostics.Process]::Start($psi)
        $peakWs = 0L
        while (-not $p.HasExited) {
            try { $p.Refresh(); if ($p.PeakWorkingSet64 -gt $peakWs) { $peakWs = $p.PeakWorkingSet64 } } catch {}
            Start-Sleep -Milliseconds 50
        }
        $p.WaitForExit()
        try { if ($p.PeakWorkingSet64 -gt $peakWs) { $peakWs = $p.PeakWorkingSet64 } } catch {}
        $mb = [math]::Round($peakWs / 1MB, 2)
        "[$m] exit=$($p.ExitCode) PeakWorkingSet=${mb} MiB ($peakWs B)" | Out-File -FilePath $log -Append -Encoding utf8
        $p.StandardOutput.ReadToEnd() | Out-File -FilePath $log -Append -Encoding utf8
    }
} finally {
    $env:PATH = $origPath
}

Get-Content $log
```

---

## 5. Verification Checklist & Clean Up

- [ ] `Token parity: 61/61 exact matches (0 failed)`.
- [ ] All vector gates (`text`, `image_b140`, `image_b280` at `768d` and `256d`) show `[PASS]` and `ALL PARITY GATES PASSED.`.
- [ ] Thread sweep shows best thread count on Ryzen 7 5700U and ms/image split (`prep`, `vis`, `back`).
- [ ] PeakWorkingSet recorded separately for `single-text`, `single-image`, and `single-drop-vision` on 8 GB RAM.
- Paste the contents of `C:\emb-val\laptop-results.txt` into your review notes or `reports/WINDOWS-0B2.md`.

Clean up temporary validation files when done:
```powershell
Remove-Item -Recurse -Force "C:\emb-val" -ErrorAction SilentlyContinue
```
