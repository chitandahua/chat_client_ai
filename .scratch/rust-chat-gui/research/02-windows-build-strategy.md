# Windows Build Strategy for a Rust + Slint (winit) App

Ticket: "Windows build strategy for a Rust + Slint app"
Research date: 2026-08-08
Sources: rustup book, rustc book, Cargo book, Rust Reference, cargo-xwin README, Slint docs, winit README.

## Answer

- **Winit backend is cross-platform to Windows out of the box.** Slint's winit backend is built in by default and "supports practically all relevant operating systems and windowing systems, including macOS, Windows, Linux with Wayland and X11." Slint targets Windows 10/11 (x86-64; Win 11 also aarch64). No Windows-specific code changes are needed to run on Windows.
- **Recommended primary path: build on a real Windows machine with MSVC (`x86_64-pc-windows-msvc`).** The MSVC target is Rust Tier 1 with host tools and is the toolchain rustup installs by default on Windows. The rustc book is explicit that cross-compiling to MSVC from a non-Windows host "may be possible but is not supported" — a native Windows build is the only fully supported MSVC route. The only prerequisite on the machine is a Visual Studio install (VS 2017 minimum, "highly recommended" latest / VS 2022) to provide the MSVC linker + Windows SDK. On Windows, `cargo build --release` for the host target is the simplest form.
- **Cross-compiling from Linux — use cargo-xwin (MSVC) over mingw-GNU if you must produce an exe on the dev box.** cargo-xwin (`cargo xwin build --target x86_64-pc-windows-msvc`) wraps xwin to auto-download the MSVC CRT + Windows SDK, so no Visual Studio is needed; prerequisites are `rustup target add x86_64-pc-windows-msvc` + clang (+ `rustup component add llvm-tools` for asm deps) + ninja for cmake-based deps. GNU mingw cross-compilation IS officially supported (windows-gnu targets "support cross-compilation"), requiring a mingw-w64 toolchain (gcc/binutils/mingw-w64; MSVCRT default) + `rustup target add x86_64-pc-windows-gnu` + a `[target.x86_64-pc-windows-gnu] linker = "x86_64-w64-mingw32-gcc"` in `.cargo/config.toml`. It is "free of all MSVC licensing implications."
- **Dependency gotchas:**
  - **MSVC:** Slint's default Skia renderer requires the MSVC toolchain (VS 2022) on Windows and has clang-cl/Skia build quirks (see Details). MSVC builds also dynamically depend on the VC runtime by default.
  - **GNU:** The Skia renderer won't work on a windows-gnu build (it needs MSVC on Windows), so a GNU cross-build must use the FemtoVG or software renderer. Slint's FemtoVG (OpenGL) and software renderers are pure-Rust-ish and cross-compile cleanly. winit itself is pure Rust and builds for either target.
- **Spec must record these Windows concerns:**
  1. **GUI subsystem flag:** add `#![windows_subsystem = "windows"]` (or the `cfg_attr(not(debug_assertions), ...)` variant) at the crate root so the app is not a console app; accepted values are `"console"` (default) / `"windows"`. Note: with `windows`, stdout/stderr are detached (no console). Equivalent linker flag is `/SUBSYSTEM:WINDOWS`.
  2. **Runtime DLLs:** MSVC builds link the C runtime dynamically by default → the VC++ Redistributable (vcruntime140.dll etc.) must be present on the target machine. Slint's own troubleshooting: an exe that "exits immediately" on Windows is typically missing MSVC runtime libraries; install the "Microsoft Visual C++ Redistributable". The UCRT is a Windows 10+ system component (no install needed).
  3. **Standalone .exe:** build MSVC with `RUSTFLAGS='-C target-feature=+crt-static'` to statically link the CRT and get a self-contained single exe; otherwise bundle/ship the VC++ redistributable next to the exe. A windows-gnu (mingw) exe depends only on MSVCRT, which ships with Windows.
  4. **Windows SDK is build-time only** — nothing from the SDK is needed at runtime on a Win10+ machine.
  5. **Stack overflow in debug builds:** Slint documents `STATUS_STACK_OVERFLOW` on MSVC debug builds; recommended `.cargo/config.toml` has `rustflags = ["-C", "link-arg=/STACK:8000000"]` for the windows-msvc targets.

## Details

### 1. Cross-compiling from Linux: cargo-xwin vs GNU mingw

#### MSVC route (`x86_64-pc-windows-msvc`) via cargo-xwin

`rustup target add` installs only the std library for the target — "there are typically other tools necessary to cross-compile, particularly a linker" (rustup book). For MSVC from Linux, that linker/CRT/SDK gap is what cargo-xwin fills.

cargo-xwin README (rust-cross/cargo-xwin):
- "Cross compile Cargo project to Windows msvc target with ease using xwin ... or windows-msvc-sysroot."
- Usage: `rustup target add x86_64-pc-windows-msvc`, then `cargo xwin build --target x86_64-pc-windows-msvc`.
- Prerequisites: clang (brew install llvm on macOS); for assembly dependencies, `rustup component add llvm-tools` or install LLVM — "A full LLVM installation is recommended to avoid possible issues."
- It auto-downloads and caches the Microsoft CRT and Windows SDK (default backend `clang-cl`; can switch to `clang`). Offline/CI caching via `cargo xwin cache xwin` (clang-cl backend) or `cargo xwin cache windows-msvc-sysroot` (clang backend). Tunable via `XWIN_*` env vars (arch, variant desktop/onecore, SDK/CRT versions, etc.).
- CMake support: for crates using the `cmake` crate, cargo-xwin generates a CMake toolchain file automatically, but "**ninja is required**".
- Legal note: "By using this software you are consented to accept the license at go.microsoft.com/fwlink/?LinkId=2086102" (MS EULA for the CRT/SDK download).
- Can run the resulting exe under wine (`cargo xwin test --target x86_64-pc-windows-msvc`).

Why Rust itself doesn't bless this: the rustc book `*-pc-windows-msvc` page states "Cross-compilation from a non-Windows host to a `*-windows-msvc` target *may* be possible but is not supported." cargo-xwin is the community-standard workaround.

#### GNU route (`x86_64-pc-windows-gnu`) with mingw

rustc book `windows-gnu` page:
- "Unlike their MSVC counterparts, windows-gnu targets support cross-compilation and are free of all MSVC licensing implications."
- "Rust does ship a pre-compiled std library for those targets. That means one can easily compile and cross-compile for those targets from other hosts if C proper toolchain is installed."
- Tested baseline toolchain: GNU Binutils 2.44, GCC 14.2, mingw-w64 12.0.0, "MSVCRT library as the default". (Older Binutils known to have issues.)
- Target is Tier 1 (`x86_64-pc-windows-gnu`).

Mechanically that means, on the Linux box: install the mingw-w64 cross compiler (Debian/Ubuntu: `gcc-mingw-w64-x86-64`), `rustup target add x86_64-pc-windows-gnu`, then configure the linker per target in `.cargo/config.toml`:

```toml
[target.x86_64-pc-windows-gnu]
linker = "x86_64-w64-mingw32-gcc"
```

The Cargo book documents the mechanism: `[target.<triple>] linker = "…"` passes `-C linker` to rustc; `[target.<triple>] rustflags`, `runner` are also available. Build with `cargo build --target x86_64-pc-windows-gnu`.

#### Which Slint/winit dependency issues each hits

- **winit**: "Window handling library in pure Rust"; no MSVC-only requirement — it compiles for both windows-msvc and windows-gnu targets. Neither route has a winit-specific blocker.
- **Slint renderer choice is the real determinant.** Slint's Skia renderer (preferred/first-tried when compiled in) on Windows requires MSVC: Slint's "Troubleshooting Skia" says the Windows build "requires the use of Microsoft Visual Studio 2022 as compiler" (an LNK2019/link.exe error is documented otherwise), and there is a known clang-cl `cannot specify '/Fo…' when compiling multiple source files` error when the cargo path contains spaces (fix: `CARGO_HOME` to a space-free path). So:
  - **MSVC/cargo-xwin route:** Skia can build but is the heaviest part of the build and the source of the known compile/link quirks. FemtoVG (OpenGL) and the software renderer are the lighter options.
  - **GNU/mingw route:** Skia is effectively off the table (needs MSVC on Windows). Use the **FemtoVG** renderer (`renderer-femtovg`) or **software** renderer. FemtoVG renders via OpenGL (or Metal/Vulkan/Direct3D with `renderer-femtovg-wgpu`); the software renderer needs no GPU but lacks some features (no rotation/scaling, no drop-shadow, border-radius+clip limitation, western-script-only text). These are the de-facto choice for GNU cross-builds.

### 2. Building on a Windows machine with MSVC

Sanest path, because:
- `x86_64-pc-windows-msvc` is Tier 1 with host tools ("guaranteed to work"; official binary releases; CI-tested after every change) — rustc book Platform Support. It is also the host toolchain rustup installs by default on Windows, so `cargo build --release` (no `--target`) targets it.
- The rustc book explicitly does not support non-Windows→MSVC cross-compilation, so a Windows host is the only supported MSVC build.
- Required on the machine: Visual Studio (VS 2017 minimum per rustc docs, "highly recommended" the latest, i.e. VS 2022), providing the `link.exe` linker and the Windows SDK headers/libs. Slint's Skia renderer specifically wants VS 2022.
- Caveat to record: MSVC's main-thread stack default is small; Slint docs recommend `/STACK:8000000` via `.cargo/config.toml` rustflags for both `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc` to avoid `STATUS_STACK_OVERFLOW` in debug builds.

### 3. Windows-specific concerns a spec must record

1. **No-console GUI subsystem.** By default a Windows app gets a console window (MSVC `link.exe` defaults to the console subsystem). Slint's Desktop page shows the fix:
   ```rust
   #![windows_subsystem = "windows"]            // always
   // or, keep console output in debug builds:
   #![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
   ```
   Rust Reference (runtime): the attribute is applied to the crate root, accepted values `"console"` (default) / `"windows"`, ignored on non-Windows targets and non-`bin` crate types. With `"windows"` the process "runs detached from any existing console" — stdout/stderr no longer appear (Slint notes you can call `FreeConsole()` to get them back). Equivalent MSVC linker flag: `/SUBSYSTEM:WINDOWS`.
2. **Runtime DLLs (MSVC).** Rust links the C runtime dynamically by default (Rust Reference, "Static and dynamic C runtimes"): "Typically targets are linked dynamically by default." On MSVC that means the binary imports the VC++ runtime (`vcruntime140.dll`, `msvcp140.dll`, …) and the UCRT. The UCRT ships with Windows 10+; the VC runtime comes from the "Microsoft Visual C++ Redistributable", which is exactly what Slint points to: an exe that "exits immediately" on Windows is "possibly caused by missing MSVC runtime libraries. To solve this install the Microsoft Visual C++ Redistributable package."
3. **Standalone .exe.** For a truly self-contained single-file exe, link the CRT statically: `RUSTFLAGS='-C target-feature=+crt-static' cargo build --release --target x86_64-pc-windows-msvc` (Rust Reference documents this exact command). Otherwise the release exe is standalone code-wise but requires the VC++ redistributable on target machines — either ship the installer or copy the needed VC runtime DLLs next to the exe. A windows-gnu build links against MSVCRT, which is present on all Windows versions, so it needs no extra runtime install.
4. **Windows SDK — build-time only.** The SDK (headers/libs for winit/Win32 API) is consumed at compile/link time. Nothing from the SDK is required at runtime on Windows 10+.
5. **Packaging.** `cargo build --release` already yields a single `.exe` (PE) for either target; packaging is just shipping that exe (plus, for MSVC, either static CRT or the VC redist). No other runtime framework is needed for the winit backend (no Qt).

### 4. Is the default winit backend cross-platform to Windows out of the box?

Yes.
- Slint "Backends & Renderers": the **winit backend is built-in by default** and is selected at startup after `qt` and before `linuxkms`. On a stock Windows machine Qt isn't installed, so winit is the effective default backend.
- Slint "Winit Backend" page: "The Winit backend uses the winit library … supports practically all relevant operating systems and windowing systems, including macOS, Windows, Linux with Wayland and X11."
- Slint "Desktop" page: runs on Windows 10 (x86-64) and Windows 11 (x86-64, aarch64); Linux is the only platform with extra runtime deps (X11/Wayland libraries), which do not apply to Windows.
- winit's own README: "Cross-platform window creation and management in Rust", pure Rust.
- Only Windows-relevant runtime requirement is the renderer's graphics API (OpenGL/Direct3D for FemtoVG/Skia, or none for the software renderer).

## Sources

Primary sources consulted (all fetched 2026-08-08):

- rustup book — Cross-compilation: https://rust-lang.github.io/rustup/cross-compilation.html
- rustc book — Platform Support (tiers incl. `x86_64-pc-windows-msvc`/`x86_64-pc-windows-gnu`): https://doc.rust-lang.org/nightly/rustc/platform-support.html
- rustc book — `*-pc-windows-msvc` target page (VS requirement; "cross-compilation from a non-Windows host … may be possible but is not supported"): https://doc.rust-lang.org/nightly/rustc/platform-support/windows-msvc.html
- rustc book — `*-windows-gnu` target page (cross-compilation supported; mingw-w64/GCC/Binutils baseline; MSVCRT default): https://doc.rust-lang.org/nightly/rustc/platform-support/windows-gnu.html
- Cargo book — Configuration (`[target.<triple>] linker` / `rustflags` / `runner`): https://doc.rust-lang.org/cargo/reference/config.html
- Rust Reference — `windows_subsystem` attribute: https://doc.rust-lang.org/reference/runtime.html
- Rust Reference — Static and dynamic C runtimes (`+crt-static` / `-crt-static`; MSVC links dynamically by default): https://doc.rust-lang.org/reference/linkage.html
- cargo-xwin README (rust-cross/cargo-xwin): https://github.com/rust-cross/cargo-xwin
- Slint docs — Desktop (Windows support, console window / `windows_subsystem`, MSVC stack-size fix): https://slint.dev/latest/docs/slint/guide/platforms/desktop/
- Slint docs — Backends & Renderers (winit built-in by default; Skia troubleshooting incl. VS2022/MSVC requirement, missing-VC-runtime instant-exit, CARGO_HOME spaces): https://slint.dev/latest/docs/slint/guide/backends-and-renderers/backends_and_renderers/
- Slint docs — Winit Backend (Windows/macOS/Linux support; renderer selection): https://slint.dev/latest/docs/slint/guide/backends-and-renderers/backend_winit/
- winit README (rust-windowing/winit; pure Rust, cross-platform): https://github.com/rust-windowing/winit

(Note: Slint "next-version" docs served at those URLs at research time; content is stable across the released docs at docs.slint.dev.)
