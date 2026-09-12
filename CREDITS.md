# Credits

Argentum is other people's work with additions on top.

Two lists live in the app — `src/argentum/credits.ts`, which is Argentum's own,
and RapidRAW's inherited Special Thanks under *Settings -> About*. This file is
the whole picture in one place, because a reader on GitHub should not have to
install the app to find out whose work this is.

**The rule:** a project is credited when something of theirs is actually
shipped — not when we plan to take it, and not when we merely admired it. When
what we took changes, the wording changes with it.

---

## Built on

### [RapidRAW](https://github.com/CyberTimon/RapidRAW) — Timon Käch

Argentum is a fork of RapidRAW. The interface, the GPU render pipeline, the
catalogue, masking, inpainting, export, presets, tethering and very nearly all
of the application are his.

To be blunt about the proportions: almost everything you touch in Argentum is
Timon's work. Argentum changed what happens to the colour.

Licensed AGPL-3.0, which Argentum inherits unchanged.

### [darktable](https://github.com/darktable-org/darktable)

Where Argentum's colour science comes from, and the reference it is measured
against. What has actually been taken:

| What | darktable source | Shipping? |
|---|---|---|
| Automatic white balance | `src/iop/channelmixerrgb.c`, `_auto_detect_WB()` @ `98a9ade9` | Yes |
| Sigmoid tone curve | `src/iop/sigmoid.c`, `_generalized_loglogistic_sigmoid` | Ported, then reverted — the code is in the tree at `mods/sigmoid.rs` but nothing calls it |

The sigmoid row is listed because the code is distributed, not because it runs.
Leaving it uncredited because it is currently disconnected would be the wrong
way round.

Beyond those two, darktable's *behaviour* is the yardstick — the comparison
harness in `mods/colour_compare.rs` measures Argentum against darktable renders
of the same files. Method that came from reading darktable, rather than copied
code, is noted in [CHANGELOG.md](CHANGELOG.md) where it applies.

Licensed GPL-3.0-or-later.

---

## Camera profiles

### [RawTherapee](https://github.com/RawTherapee/RawTherapee)

Argentum reads DCP camera profiles but **ships none**. The free collections are
published with their authors' individual permission rather than under a licence
that lets anyone else redistribute them — so when Argentum fetches a profile for
your camera, it comes from RawTherapee's collection, from the project that
published it, not from us.

---

## Inherited with RapidRAW

Argentum ships all of RapidRAW's functionality, so everything he depends on,
Argentum depends on too. These are his credits, repeated here because they are
just as true of this app.

| Project | What it does here |
|---|---|
| [rawler](https://github.com/dnglab/dnglab) | RAW decoding. The foundation everything else stands on |
| [lensfun](https://lensfun.github.io/) | The lens correction library and its camera/lens database |
| [SAM 2](https://github.com/facebookresearch/sam2) | The foundation model behind AI subject detection and click-to-mask |
| [U²-Net](https://github.com/xuebinqin/U-2-Net) | The architecture behind sky and foreground detection |
| [LaMa](https://github.com/advimman/lama) | Inpainting — content-aware fill and object removal |
| Depth Anything | Monocular depth estimation, which drives depth-based masking and lens blur |
| NIND | The models behind AI noise reduction |
| [NegPy](https://github.com/marcinz606/NegPy) | The approach the film-negative conversion is based on |
| Spektrafilm — Andrea Volpato | The spectrally-based film emulation LUTs. Licensed CC BY-SA 4.0 |
| [libgphoto2](https://github.com/gphoto/libgphoto2) | Camera communication for tethering and remote capture. Feature-gated: `default = []`, so it is only in builds made with `--features tethering` |

Two entries are deliberately named without a link: **Depth Anything** and
**NIND** have several plausible upstreams and none is cited in the code, so
naming them is honest where guessing a URL would not be.

---

## Licence

Argentum is licensed **AGPL-3.0** — RapidRAW's licence, inherited unchanged. See
[LICENSE](LICENSE).

darktable is GPL-3.0-or-later, which is compatible with the AGPL: the combined
work is distributed under the AGPL. Spektrafilm's LUTs are CC BY-SA 4.0 and are
attributed above as that licence requires.
