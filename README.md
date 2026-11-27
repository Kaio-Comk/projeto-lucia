# Projeto Lucia

An experimental **Xbox Series X|S → Windows x64** compatibility layer in **Rust** over **Direct3D 12**, built on
[fxceless/victory-fork](https://github.com/fxceless/victory-fork) and [Marvisak/VIctory](https://github.com/Marvisak/VIctory),
and aimed at **modest PCs**: a **4 GB** NVIDIA card, **16 GB** of RAM, a **4-core** CPU and **Windows 10**. The other
public forks are measured on 10-24 GB cards; this one is about what it takes to run on hardware many people actually have.

> **No game data here.** This repository holds no game files, executables, keys, assets, captures or download links.
> To run anything you supply your own unpacked title directory, kept outside the repository (see `docs/asset-policy.md`;
> `cargo xtask policy` checks every commit).

## Status (2026-11-27, measured on the target PC)

| Stage | State |
|-------|-------|
| Boot on Windows 10 | ✅ needs the D3D12 Agility SDK (see below): Windows 10's own runtime stops at Shader Model 6.5 |
| First-boot screens, legal and terms screens, menus | ✅ 30 fps (they were black before the interface-plane fix below) |
| Loading screens with artwork | ✅ |
| The world | 🟡 reached and running (4,000-5,700 draws a frame, the "move / move the camera" hint shows), **about 5-6 fps**; the picture is dark wherever no direct light falls |
| Playable | ❌ not yet |

Target PC: Intel Xeon E3-1270 v3 (4c/8t, Haswell) · NVIDIA GeForce GTX 1650 SUPER 4 GB (Turing, Wave32, no RT cores) ·
16 GB RAM · Windows 10 Pro 19045 · Series S profile.

Measured in the world (runs of 8-12 minutes, `scripts/executar.ps1`):

| Measure | Value |
|---------|-------|
| Frame | 300-370 ms (about 3 fps in the heaviest views, 5-6 fps on average) |
| Preparing submissions (CPU) | 140-200 ms a frame |
| GPU | 60-115 ms a frame |
| Video memory | about 3.9 GB of the card's 4 GB (Windows gives it a budget of 3.3 GB; the rest spills to shared memory) |
| Commit | 35-37 GB (a 32-48 GB pagefile on an SSD is needed) |

Known limits, honestly:
- **Shadowed areas are black.** The title's bounce light and its lamp and sun shadows are ray traced in compute
  programs that do not run on NVIDIA cards in any public build yet; a constant ambient light (`ambient.rs`) only lifts the
  floor. The first room of the game is an interior, so most of it is dark.
- **The first entry into the world must not compile shaders**: about 1,600 pipelines at once on a 4-core CPU make the
  title's own GPU watchdog end the process. Run `scripts/aquecer.ps1` once after every build (it pre-translates the
  shaders seen in earlier runs).
- The structure builder for ray tracing stays off (`VICTORY_BVH_BUILDER=0`): upstream reports a crash 30-40 s after it starts.

## What Projeto Lucia adds

- **Windows 10 support: the D3D12 Agility SDK loaded at run time** (`victory-shader/src/agility.rs`, own code from
  Microsoft's documented API). Devices, the debug layer, DRED and root-signature serialization all come from the
  Agility runtime (`ID3D12SDKConfiguration1::CreateDeviceFactory`): the inbox `D3D12SerializeVersionedRootSignature`
  refuses the Shader Model 6.6 flags on Windows 10 even with an Agility device. Folders are passed by their 8.3 name:
  a path with a space made `CreateDeviceFactory` fail (`0x887E0003`).
- **Series S menus were black: the interface plane.** A Series S title presents two planes (the game, and the interface
  at 3840x2160); only the first was shown, and the second was looked for at a 24-byte step where the retail
  PresentX description is 72 bytes. Fixed (`queue.rs` `second_plane`, composition from VItoriaBr's `compose.rs`).
- **Mip cap for 4 GB cards** (`VICTORY_MIP_CAP=<n>`, `tiles.rs`): the largest levels of big textures never get card
  memory, using the reserved-texture "levels on demand" machinery; -410 MB of live textures with `n=1` in the world.
- **Where the video memory goes**: the periodic memory line now splits committed textures by kind and says why title
  textures miss the mip cap (most of what is left: thousands of textures under 1 MB).
- **Run tooling**: `scripts/executar.ps1` turns every run into a folder with the game log, the PC's CPU/RAM/commit/disk
  every 2 s, the card's temperature/use/VRAM/clock/power every second (`nvidia-smi`), the `cargo xtask bench` summary and
  a `resumo.md` with alerts and the screens passed; `-Teclas mundo` drives a fresh save from boot to the world.
  `scripts/perfil-pc4gb.ps1` is the switch set for the target PC, shared by runs, the shader warm-up and play.
- `translate --warm <list>` names each shader it finds (its program), to tell them apart in a run's log.

## Running

```powershell
# Build (Rust and VS Build Tools with the Windows SDK)
cargo build --release -p victory

# Windows 10: the Agility SDK beside the executable (from Microsoft's NuGet package
# Microsoft.Direct3D.D3D12; never commit the DLL)
mkdir target\release\D3D12
copy <package>\build\native\bin\x64\D3D12Core.dll target\release\D3D12\

# The title directory: victory.toml (game_dir), or ..\GTA6 beside this repository for the scripts
copy victory.example.toml victory.toml

# Once after every build: pre-translate the shaders
.\scripts\aquecer.ps1

# Play (keyboard as a pad: Enter = A, Backspace = B, WASD / IJKL sticks, arrows = D-pad, Esc = Menu)
.\scripts\executar.ps1 -Nome jogar -Segundos 86400
```

## Credits

Projeto Lucia is a fork; most of the runtime is other people's work:
- [Marvisak/VIctory](https://github.com/Marvisak/VIctory) and its contributors: the loader, runtime services, Direct3D 12
  renderer and shader recompiler (GPL-3.0-or-later).
- [fxceless/victory-fork](https://github.com/fxceless/victory-fork) (hyphennn) and its contributors: the base of this
  repository, merged up to 2026-11-27 (GPL-3.0-or-later).
- [uiliannn/VItoriaBr](https://github.com/uiliannn/VItoriaBr) (Uilian): `compose.rs`, `ambient.rs` (first ported from
  Belobug) and the packed dot-product lowering.
- [Belobug/VIctory-Belobug-AMD](https://github.com/Belobug/VIctory-Belobug-AMD): the ambient light fallback.
- [avatarmanz/VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX): the retail PresentX plane layout (72 bytes),
  used here as documented fact, code written anew.
- [stellarr-studio/kelvaroq](https://github.com/stellarr-studio/kelvaroq): patches 0002, 0012, 0015, 0026, 0045 and 0063,
  rebuilt for this base.
- No code from XCL (GPL-2.0-only) is in this repository.

## License

GPL-3.0-or-later; see [LICENSE](LICENSE).
