# Hissabeth

Hissabeth is a CPU optical design workbench built with Rust, Tauri, and Vue.
It combines sequential optical design, 3D coordinate breaks, and free-space
laser bench simulation with native Damped Least Squares (DLS) optimization and
Monte Carlo tolerancing.

The desktop and MCP agents work on the same saved project. Edit the prescription
directly in the desktop app or ask an agent to analyze, optimize, and update it.
Revision checks protect the project when both are in use.

## Features

| Area                                   | Capabilities                                                                                                                                                                                                     |
| -------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Sequential optics and profiles         | Finite and infinite conjugates, spherical and aspheric surfaces, toroidal and Zernike Fringe sag, sequential mirrors, tilt and decenter coordinate breaks, AGF glasses, gap solves, and multiple configurations. |
| Analysis and diffraction               | First-order metrics, ray inspection, spot diagrams, geometric MTF, ray and OPD fans, field curvature, wavefront, FFT and Huygens PSF/MTF, Zernike analysis, and Gaussian-beam and fiber coupling.                |
| DLS optimization and tolerancing       | Numerical optimization with selectable variables and merit operands, sensitivity ranking, and seeded Monte Carlo trials with optional autofocus compensation.                                                    |
| Free-space laser bench                 | 2D table and 3D bench views, real lens groups, splitters, mirrors, detectors, branch power, obstructions, and assembly export.                                                                                   |
| Desktop workspace and revision history | Direct editing on a shared draft, current-revision analysis, multiple project files, and workspace undo, redo, and optical diffs.                                                                                |
| AI agents and MCP                      | In-app Codex and Claude Code conversations, external MCP clients through a revision-aware broker, and optional ChatGPT sign-in for text conversations.                                                           |

## Quick start

Install [Bun](https://bun.com/), Rust, and the native build toolchain for your
desktop platform. On Debian-derived Linux distributions, the Tauri build uses
these system packages:

```sh
sudo apt-get update
sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev \
    patchelf libssl-dev libgtk-3-dev
```

From the repository root:

```sh
bun install
bun run tauri dev
```

Open **Edit** to change the optical specification or surface table. Apply the
draft with **Ctrl+S**, then open **Optical layout** to see the saved prescription
or the free-space bench. Analysis views run against the saved revision.

Use native Hissabeth JSON to retain the entire project. Use ZMX or Geopter JSON
for sequential exchange within the documented format limits.

### Use an agent in the desktop

1. Install Node.js 22 or later and an authenticated Codex or Claude Code CLI.
2. Open **Assistant** and choose the agent and model.
3. Apply or revert any outstanding desktop draft before sending your request.

For example, ask the agent to summarize the current prescription, optimize a
selected variable for a target focal length, and compare the saved revision
against the previous one. The desktop refreshes its saved views after MCP writes.

To connect an external Codex or Claude Code client, open
**Tools → Agent connections** and select the installed clients to register.

### Run without the desktop

Build the headless executable and keep the broker running:

```sh
cargo build --manifest-path src-tauri/Cargo.toml --no-default-features
src-tauri/target/debug/hissabeth --engine
```

Configure the MCP client to launch the same executable with `--mcp` for stdio.
For example, a client with an `mcpServers` configuration can use:

```json
{
    "mcpServers": {
        "hissabeth": {
            "command": "/absolute/path/to/src-tauri/target/debug/hissabeth",
            "args": ["--mcp"]
        }
    }
}
```

Set the same `HISSABETH_DATA_DIR` in both processes to choose a workspace.
From MCP, call `workspace_status` first and `system_get` before editing.
Use the latest revision for every write.

### Build the desktop application

```sh
bun run tauri build
```

Tauri builds the frontend and agent bridge into `dist` and places native build
outputs in `src-tauri/target` by default.

## Attribution

The optical implementation draws on
[heterophyllus/Geopter](https://github.com/heterophyllus/Geopter), commit
`0edfbf52fcf0e3fc660e5ae33c354fd7a6be80d7`. Geopter is Copyright (C) 2021
Hiiragi. Its GPL license is retained in [LICENSE.md](LICENSE.md).

## Development checks

The [CI workflow](.github/workflows/ci.yml) runs on pushes to `main` and pull
requests. It tests the headless Rust library, checks the Linux desktop target, and
runs the frontend typecheck, lint, formatting check, and build.

```sh
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --lib
bun run typecheck
bun run lint
bun run format:check
bun run build
```

### Optical reference validation

The [oracle corpus](src-tauri/tests/oracle/cases) currently passes **54/58 checks
across 12 cases** in Rust and through `hissabeth --engine --run`. Four Geopter
pupil comparisons are expected failures. The corpus also covers analytic thin
and thick lenses, a cemented doublet, mirrors, and published prescriptions from
[Kingslake and Johnson, §2.5](https://www.sciencedirect.com/science/article/pii/B9780123743015000061)
and [Sasián's Cooke triplet lecture](https://wp.optics.arizona.edu/jsasian/wp-content/uploads/sites/33/2016/03/L20_OPTI517_Cooke_triplet.pdf).
The literature focal lengths use the published precision, and the Petzval sums
use the published radii with the reference project's d-line indices.

If `reference/upstream` is absent, fetch the pinned prescriptions:

```sh
git clone --filter=blob:none --no-checkout https://github.com/heterophyllus/Geopter reference/upstream
git -C reference/upstream sparse-checkout set example
git -C reference/upstream checkout --detach 0edfbf52fcf0e3fc660e5ae33c354fd7a6be80d7
```

Run the corpus in-process, or add the headless CLI run with the second command:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test oracle
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --test oracle oracle_ -- --include-ignored --nocapture
```

Each case accepts an inline native `prescription.system` or a Geopter
`prescription.file` relative to `reference/upstream`. Checks use the tool's
arguments. A `geometric_mtf` check adds `frequency` in cycles/mm to select a
sample. `exact` compares counts and strings directly. `tight` defaults to
`abs = 1e-12, rel = 1e-9`, and `loose` to `abs = 1e-6, rel = 1e-3`.
The numeric limit is `max(abs, rel * |expected|)` with per-check overrides.
Unexpected failures and unexpected passes of `xfail` checks fail the run.

The Geopter expectations are frozen snapshots of the upstream C++ results at
the pinned commit; the corpus checks them against this engine only.

## Releases

The [Release workflow](.github/workflows/release.yml) builds tagged installers:

| Platform | Architecture          | Assets                      |
| -------- | --------------------- | --------------------------- |
| Linux    | x64                   | AppImage and Debian package |
| Windows  | x64                   | MSI installer               |
| macOS    | Apple Silicon (arm64) | DMG disk image              |
| macOS    | Intel (x64)           | DMG disk image              |

The version tag must match the version in `package.json`,
`src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`. Commit the source and
lockfiles before tagging.

```sh
git tag v0.1.0
git push origin v0.1.0
```

The workflow uploads installers to a draft and publishes it after all matrix
builds succeed. A failed build leaves the draft available for a rerun. Tags
such as `v0.1.0-rc.1` publish prereleases. Windows signing and macOS signing
and notarization are currently unconfigured.

## Name and icon

The project name and the application icon are unofficial fan work based on a
character from _Reverse: 1999_. All rights in the character, its name, and the
related artwork belong to Bluepoch (© BLUEPOCH CO., LTD.). Hissabeth is a
non-commercial project and is not affiliated with, endorsed by, or sponsored by
Bluepoch.

The icon files in `public/` and `src-tauri/icons/` are not covered by the
project's license. They will be removed or replaced at the rights holder's
request.

## License

Hissabeth is distributed under the GNU General Public License, version 3 or
later, except for the icon files described in [Name and icon](#name-and-icon).
[License text](LICENSE.md)
