# 05 — Windows build

**What to build:** The app builds and runs as a Windows desktop app. Concretely: `#![windows_subsystem = "windows"]` so it launches without a console, the Slint renderer feature set chosen so GNU cross-builds work (FemtoVG or software renderer — Skia needs MSVC), and a short build note recording the three supported paths and their needs (native MSVC with VS2022; cargo-xwin; GNU mingw cross) plus VC++ Redistributable vs `-C target-feature=+crt-static` for a standalone exe and the `/STACK:8000000` msvc rustflag. Verifiable by building/running the exe on a Windows machine.

**Blocked by:** 01 (Scaffold + Slint window boots).

**Status:** ready-for-agent

- [ ] App builds for `x86_64-pc-windows-msvc` (native Windows, VS2022) without console window
- [ ] Renderer feature selection supports a GNU/mingw cross-build (no Skia dependency)
- [ ] Build note in the repo records the Windows build paths, VC redist vs `+crt-static`, and `/STACK` rustflag
- [ ] Linux build is unaffected (still `cargo run` clean)
