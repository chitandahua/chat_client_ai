# Windows build strategy for a Rust + Slint app

Blocked by:
Type: research
Status: resolved

## Question

What is the current recommended way to produce a Windows build of a Slint desktop app from a Linux dev box (cargo-xwin / mingw cross-compile) versus building on a Windows machine — and what does each require for a Slint + winit app? What's the minimal set of Windows-specific concerns (dependencies, DLLs, linkers) the spec must record so the "Linux + Windows both run" goal (Q7) is actually met?

## Answer

Full findings: `.scratch/rust-chat-gui/research/02-windows-build-strategy.md` (sources: rustc/rustup books, cargo-xwin, Slint docs, winit, 2026-08-08).

- **Winit is Windows-ready out of the box** — Slint's winit backend is the default on Windows (10/11 x86-64); no platform-specific code needed. The renderer choice is the real determinant.
- **Recommended build path: real Windows machine, MSVC** (`x86_64-pc-windows-msvc`, Tier 1) — `cargo build --release` with VS 2022 installed. Non-Windows→MSVC cross-compile is unsupported by rustc, so from Linux only: **cargo-xwin** (auto-downloads MSVC CRT + Windows SDK; needs clang/llvm-tools/ninja) or **GNU mingw** (`x86_64-pc-windows-gnu`, officially cross-compilable; needs mingw-w64 + `.cargo/config.toml` linker `x86_64-w64-mingw32-gcc`).
- **Renderers**: GNU cross-builds can't use Skia (needs MSVC) → use **FemtoVG** or software renderer. MSVC can use any.
- **Spec must record**: `#![windows_subsystem = "windows"]` (no console); MSVC builds need the VC++ Redistributable at runtime (or `RUSTFLAGS='-C target-feature=+crt-static'` for a self-contained exe; windows-gnu needs no runtime install); SDK is build-time only; `/STACK:8000000` rustflags on msvc targets to avoid debug stack overflow.
