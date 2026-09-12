# Argentum

**A RAW photo editor.** RapidRAW's interface and AI, darktable's colour science —
harvested one module at a time.

Argentum is a fork of [RapidRAW](https://github.com/CyberTimon/RapidRAW) by Timon
Käch. His is the interface, the GPU pipeline, the catalogue and very nearly all of
the application. Argentum's own work is the colour: what happens to a RAW file
between the sensor data and the picture on screen, rebuilt on
[darktable](https://github.com/darktable-org/darktable)'s methods.

Everything Argentum adds lives in its own files, so RapidRAW's updates keep
merging cleanly. That constraint is enforced by a script, not by good intentions
— see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

---

## Status

Builds and runs, and is used daily on Windows.

| Platform | State |
|---|---|
| **Windows** | Tested. The platform it is developed on |
| **macOS** | Builds in CI, never run by anyone. Reports welcome |
| **Linux** | Builds in CI, never run by anyone. Reports welcome |

It is cross-platform because RapidRAW is, and nothing Argentum added breaks that.
But "compiles" is not "works", and nobody has checked.

---

## Install

Grab an installer from [**Releases**](https://github.com/akhater/Argentum/releases)
— Windows `.exe`, macOS `.dmg`, Linux `.AppImage` and `.deb`.

Nothing else to install. The AI models are downloaded on first use, not shipped
in the binary, so the download stays small.

*(Building from source is further down, and only needed if you want to change
the code.)*

---

## What Argentum adds

Everything here is about colour and RAW decoding. The editing interface is
RapidRAW's.

- **White balance rebuilt on real colour science.** Kelvin and tint, scene-linear
  Bradford adaptation, and a picker that settles on one answer instead of drifting
  each time you click.
- **Automatic white balance**, ported from darktable's illuminant detection.
- **Camera profiles.** A profile describes how your particular camera sees colour.
  Argentum reads DCP profiles, fetches one for your body, and keeps the cameras
  you shoot with under *My Gear*.
- **Highlight recovery.** When a bright area clips in one channel, the two that
  survived say what colour it was — so the channel is rebuilt before demosaic
  rather than smeared to white.
- **A clipping warning that steps through the channels** — off, luminance, R, G, B
  — so you can see which channel is about to lose everything.
- **Display colour management.** The preview is converted for the monitor's own
  ICC profile, so what you see matches what you export. Windows only for now; see
  Known limitations.
- **Canon sRAW and mRAW fixed.** They came out green with crushed shadows. Both
  causes found and corrected.
- **Automatic lens detection on Canon bodies**, read from the MakerNote, so lens
  corrections apply without picking the lens by hand.
- **An RGB readout**, so a colour under the cursor can be measured instead of
  argued about.

Full detail, with the darktable source file and commit for every harvested
module, is in [CHANGELOG.md](CHANGELOG.md). What is planned is in
[docs/ROADMAP.md](docs/ROADMAP.md).

---

## Known limitations

**The screen conversion is Windows-only, and needs a matrix profile.** Argentum
reads the ICC profile Windows holds for the monitor the window is on and converts
the preview for it. On macOS and Linux nothing is read and nothing is converted,
and the same is true of a monitor profile built as a lookup table rather than
from three primaries. In those cases the picture is shown the way RapidRAW always
showed it — correct on an sRGB screen, over-saturated on a wide-gamut one.

**macOS and Linux are untested.** As above: built, never run.

The app also lists what is currently broken in *Settings → About → Known issues*,
which is kept current rather than being a changelog entry that goes stale.

---

## Where the app keeps its files

In the operating system's own application-data folder, under
`co.argentum.editor`:

| | Windows | macOS | Linux |
|---|---|---|---|
| Settings, AI models, presets | `%APPDATA%\co.argentum.editor` | `~/Library/Application Support/co.argentum.editor` | `~/.config/co.argentum.editor` |
| Thumbnail cache | `%LOCALAPPDATA%\co.argentum.editor` | `~/Library/Caches/co.argentum.editor` | `~/.cache/co.argentum.editor` |

**This is not configurable yet.** A portable, one-folder installation is on the
roadmap and is not built.

The one thing kept outside: `.agdata` edit files sit next to your photos, so your
edits travel with the pictures rather than living in a database you have to back
up separately.

---

## Building from source

Only needed if you want to change the code. You need Rust, Node, and a C++
toolchain.

On Windows:

```bash
winget install Microsoft.VisualStudio.2022.BuildTools --override "--quiet --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

```bash
winget install Rustlang.Rustup
```

```bash
winget install OpenJS.NodeJS.LTS
```

On macOS install Xcode command line tools, Rust via [rustup](https://rustup.rs)
and Node; on Linux, your distribution's `webkit2gtk` development packages plus
the same two.

Then, on any of them:

```bash
npm install
```

Run a development window that reloads as you edit:

```bash
npm run tauri dev
```

Build an installer for the machine you are on:

```bash
npm run tauri build
```

---

## Staying current with RapidRAW

```bash
git remote add upstream https://github.com/CyberTimon/RapidRAW.git
```

```bash
git fetch upstream && git merge upstream/main
```

This is meant to be boring, and it is the reason for the architecture. If a merge
needs real thought rather than "keep both sides", something drifted into a file
it should not be in — see
[ARCHITECTURE.md](docs/ARCHITECTURE.md).

---

## Docs

| File | What's in it |
|---|---|
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Code layout, and why upstream updates don't hurt |
| [docs/ADDING_A_TOOL.md](docs/ADDING_A_TOOL.md) | The recipe for harvesting a module |
| [docs/ROADMAP.md](docs/ROADMAP.md) | What is planned, in what order |
| [docs/MENU.md](docs/MENU.md) | Everything available to harvest, from each source |
| [docs/FEASIBILITY.md](docs/FEASIBILITY.md) | The original go/no-go research |
| [CHANGELOG.md](CHANGELOG.md) | Every module, with its upstream source and commit |
| [CREDITS.md](CREDITS.md) | Whose work this is built on |

---

## Credits and licence

Argentum is other people's work with additions on top. **[CREDITS.md](CREDITS.md)**
names them, and the same list is in the app under *Settings → About*.

Licensed **AGPL-3.0**, inherited from RapidRAW and unchanged. See
[LICENSE](LICENSE).
