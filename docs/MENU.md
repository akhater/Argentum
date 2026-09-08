# The menu — what you can take from each side

Pick whatever you want, whenever you want. Nothing here depends on anything
else except the color conversion (built once, during the first pick).

Difficulty: 🟢 = 1–2 days · 🟡 = ~a week · 🔴 = needs a real decision first

---

## Take from darktable

| What | Why it's better | Effort |
|---|---|---|
| **White balance** | Real chromatic adaptation vs RapidRAW's 3 fudge numbers | 🟢 |
| **Highlight recovery** | 4 real methods for blown skies. RapidRAW basically clips | 🟢 |
| **Filmic / Sigmoid / AgX** | Proper highlight rolloff — the "why does darktable look nicer" thing | 🟢 |
| **Color balance rgb** | Best color grading in any free RAW app | 🟢 |
| **Color equalizer** | Per-hue control that doesn't fall apart on skin | 🟢 |
| **Diffuse or sharpen** | Unique to darktable. Sharpening, lens blur recovery, denoise in one | 🟡 |
| **Local laplacian** | Local contrast without halos | 🟡 |
| **Profiled denoise** | Uses real measured noise profiles per camera model | 🟡 |
| **Demosaic (RCD / Markesteijn)** | Matters a lot if you shoot Fuji X-Trans | ⚪ |
| **Lens correction from embedded RAW data** | Rescues lenses lensfun doesn't know | ⚪ |

⚪ = parked. Real work, no benefit *here* — see
[ROADMAP](ROADMAP.md#someday--only-if-Argentum-is-ever-open-sourced). Both would
matter for a public release.

**Lens correction — read this before assuming it's missing.** RapidRAW already
has a complete lens correction module: EXIF auto-detect, manual mode, a "my
lenses" shortlist, distortion / CA / vignetting, and the full 5MB lensfun
database bundled. Every lens in the current kit is in it, plus the 5D Mark II
body. **It works on day one.**

The only thing absent is reading correction data the *camera* embeds in the RAW.
That exists to rescue glass lensfun has never heard of — mostly recent
mirrorless. Nothing in the current kit qualifies.

**Demosaic — why parked.** It sits earlier in the chain than everything else, so
it means touching how RAW files get decoded. A design decision rather than a
copy. And the win is mostly Fuji X-Trans; the 5D Mark II is conventional Bayer.

---

## Keep from RapidRAW

Already there. Nothing to do.

- **AI masking** — subject, sky, foreground, depth. darktable has nothing close
- **Generative AI remove / replace**
- **The interface and the speed** — the actual reason to start from here
- **Tethering** (shoot straight into the app)
- **Culling** (fast keep/reject passes)
- **Panorama, HDR merge, focus stacking**
- **Presets, auto-tagging**

---

## How the edits are saved

JSON sidecar next to each photo. Original RAW never modified.

```
IMG_1234.CR3          ← untouched
IMG_1234.CR3.agdata   ← your edits, readable text
```

Supports multiple versions of the same photo (`IMG_1234.CR3.2.agdata`).

Every tool you add writes into the same file. Doesn't matter whether a setting
came from darktable or RapidRAW — one list, one format. That's what kills the
"can't continue an edit in the other app" problem.

---

## Suggested order

Not a rule — just the cheapest path.

1. **White balance** — builds the color conversion everything else reuses
2. **Highlight recovery** — biggest visible save on real photos
3. **Filmic or Sigmoid** — sets the overall look
4. Then whatever you feel like

Anything after step 1 is roughly the same job each time.

Stop whenever. A version with only steps 1–2 in it is a working app.
