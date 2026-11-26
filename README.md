# Projeto Lucia (Lucia Runtime)

An experimental **Xbox Series X|S → Windows x64** compatibility layer written in **Rust** on top of **Direct3D 12**, tailored specifically for **entry-level and mid-range PCs (4 GB VRAM, 16 GB system RAM, 4-core CPUs)** rather than high-end 16–24 GB GPUs.

> **Important:** This repository contains **zero game files, executables, keys, assets, captures, or download links**. To run anything, you must supply your own lawfully obtained, unpacked title directory kept completely outside this repository (`docs/asset-policy.md`).

---

## Scope & Architecture

Console titles compiled for Xbox Series x86-64 execute natively on Windows CPUs, while the runtime replaces console-only OS and graphics interfaces:

- **PE Loader & Console HLE:** `kernelx`, `xgameruntime`, memory management, storage queues, audio, and input.
- **RDNA2 Shader Recompiler:** translates console RDNA2 programs into **DXIL** for host D3D12 pipelines (`d3d12_xs` → PC Direct3D 12).
- **Low-VRAM / Low-RAM Focus:** reduces duplicate CPU/GPU memory footprints, constrains staging and texture budgets, and targets **Series S** rendering profiles with ray tracing disabled.

---

## Target Hardware Profile

This project is developed and profiled around modest desktop hardware:

| Component | Specification | Notes |
| --- | --- | --- |
| **CPU** | Intel Xeon E3-1270 v3 @ 3.50 GHz (4c / 8t, Haswell) | AVX2 supported; 4 cores require minimal submission thread overhead |
| **GPU** | NVIDIA GeForce GTX 1650 SUPER (4 GB VRAM, Turing) | Shader Model 6.6 + Mesh Shaders Tier 1, Wave32, no RT Cores, strict 4 GB VRAM ceiling |
| **RAM** | 16 GB system memory | Requires SSD pagefile headroom and reduced CPU/GPU shadow duplication |
| **OS** | Windows 10 Pro x64 (Build 19045) | — |

---

## Current Status

**Early setup / baseline initialized.** No boot, frame-time, or gameplay claims are reported yet until verified locally on the target machine after toolchain setup and testing.

| Stage | Status | Notes |
| --- | --- | --- |
| Workspace & clean GPL-3.0 baseline | ✅ Ready | Initialized from `victory-fork` without third-party logs |
| Local build & unit tests (`cargo test`) | ⏳ Pending | Requires Rust MSVC toolchain installation |
| Hardware smoke test (GTX 1650 SUPER 4 GB) | ⏳ Pending | Awaiting title extraction and first run logs |
| Low-VRAM mip-cap & memory-pressure layer | ⏳ Planned | Core differentiator under design |

---

## Planned Engineering Work

What sets **Projeto Lucia** apart from existing high-VRAM forks:

1. **4 GB VRAM Texture Mip-Cap & Eviction:** trimming top mip levels (`mip 0` / `mip 1`) on large 2K/4K textures during creation/upload to cut texture VRAM usage by ~75% and prevent `E_OUTOFMEMORY` / TDR resets.
2. **16 GB RAM Residency & Zero-Copy Paths:** adapting commit/RAM pressure monitoring (`GlobalMemoryStatusEx`) and direct CPU-written constant heap reads (`OpenExistingHeapFromAddress1`) to minimize duplicate host buffers.
3. **NVIDIA Wave32 & No-RT Compatibility:** paired Wave64 → Wave32 compute support, safe barrier lowering, LDS bounds checks, and lightweight ambient lighting fallback without ray tracing.

---

## Building from Source

### Prerequisites
- Windows 10 / 11 x64
- CPU with **AVX2**
- Direct3D 12 GPU supporting **Shader Model 6.6** and **Mesh Shaders**
- **Rust** (`x86_64-pc-windows-msvc`, pinned in `rust-toolchain.toml`)
- **Visual Studio Build Tools** (Desktop development with C++, x64 MSVC toolset, Windows 10/11 SDK)

### Commands
```powershell
# 1. Configure external paths (kept outside git)
copy victory.example.toml victory.toml

# 2. Verify workspace tests (no game files required)
cargo test --workspace

# 3. Build release binary
cargo build --release -p victory
```

---

## Credits & License

- Licensed under **GPL-3.0-or-later** — see [LICENSE](LICENSE).
- Built upon **[fxceless/victory-fork](https://github.com/fxceless/victory-fork)** and **[Marvisak/VIctory](https://github.com/Marvisak/VIctory)** (both GPL-3.0-or-later).
- **License boundary:** No **GPL-2.0-only** code from XCL is copied into this repository.
