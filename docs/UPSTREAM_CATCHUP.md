# RapidRAW catch-up decisions

This is the handoff for future upstream reviews. Read it alongside
`scripts/upstream-decisions.mjs` and the mergeability rules in
[ARCHITECTURE.md](ARCHITECTURE.md). A clean Git merge does not establish that
image processing still behaves correctly.

## 2026-09-19: split the highlight boundary by responsibility

**Decision:** adopt RapidRAW's scene-linear headroom change, keep Argentum's
pre-demosaic RAW recovery, remove the obsolete highlight-compression operation,
and do not stack RapidRAW's overlapping post-demosaic highlight recovery on top.

The application processing stages floor intermediate RGB at zero without an
upper clamp at `1.0`. RAW decode also retains above-white values before inverse
sRGB conversion. Values above nominal white therefore survive until the later
display/output mapping. This is deliberately separate from RapidRAW's
`raw_processing.rs` post-demosaic recovery algorithm.

Argentum's `mods::highlights::recover` remains the recovery authority. It runs
on the decoded CFA data before demosaicing and uses measured unclipped channel
ratios. RapidRAW's post-demosaic RGB correction is not imported because it
could stack on pixels already reconstructed by Argentum. The legacy
`raw_highlight_compression` setting remains readable and is preserved on save,
but no longer drives a compression calculation. Full-quality decode retains
the upstream 1000.0 scene-linear ceiling; fast/thumbnail decode retains its
existing 1.0 ceiling.

Numerical regression tests cover preservation of values above `1.0` and the
continued lower floor at zero. This boundary was reconciled as part of the
v1.6.4 catch-up below.

## 2026-09-22: RapidRAW v1.6.4 catch-up decisions

The isolated branch `codex/rapidraw-1-6-4-catchup` reviews the exact upstream
range `40cfa3df..71a07921`. The overlap register contains one explicit
decision for every detected overlap (212 entries). The required upstream
review command and mergeability check are part of the final verification.

- Adopt the final Brightness implementation from `86884cc9`, which applies
  Brightness to `base_srgb` after tone mapping. Do not use the superseded
  intermediate shader implementations.
- From `85bf424a`, retain above-white LinearRaw values before inverse sRGB
  conversion and remove the old compression calculation. Keep Argentum's
  pre-demosaic recovery and do not import the overlapping post-demosaic
  recovery. Keep the legacy compression setting only for migration.
- Keep full-quality's 1000.0 ceiling and fast/thumbnail's 1.0 ceiling.
- Preserve Argentum's TIFF rendering implementation: a 32-bit-float output
  target followed by 16-bit integer TIFF quantization. Keep bit depth as the
  existing global preference, not a per-preset property. Add headless
  `--tiff-bit-depth` (8 or 16, default 16; invalid values fail before startup)
  and remove the redundant GPU readback flag.
- Adopt the reviewed neutral-grey canvas, persistent Quick Filter, Vibrance,
  RGB curves, folder-tree sizing, labels, and final Android workflow changes;
  keep Argentum branding and version identity.
- For rawler, preserve Argentum's `94818b0` Canon EOS C50 data and layer the
  upstream `934af4b` negative-only clipping change on top. This is decoder
  synchronization, not a proven R6 III fix: Argentum's `u32::MAX` white level
  may mean the changed clipping function does not receive values above 1.0.
  The port is committed locally as `34eeaadc` on companion branch
  `codex/c50-1-6-4-sync`. Argentum's lockfile intentionally still points at
  `94818b0`: the companion commit is not published, so switching the lockfile
  now would make other checkouts and CI unable to fetch it. Activating this
  decoder sync requires review, then user approval to publish the companion
  branch and update Argentum's lockfile.

Validation must distinguish automated checks from the requested camera
comparison. The exact R6 III CR3 and matching DPP reference were not available
in this worktree, so no visual/A-B conclusion is claimed. The planned matrix
remains: base vs. base+`934af4`, old vs. v1.6.4 ceiling, Argentum recovery
on/off, full vs. fast decode, neutral and edited Brightness/Highlights, editor
vs. export. Compare identical input/settings at equal-sized 100% output with
cold caches, and record whether rawler's function receives above-one values.

## 2026-09-22: preserve Argentum's TIFF precision implementation

**Decision:** keep Argentum's separate 32-bit-float TIFF render path. Do not
replace it with RapidRAW PR [#1466](https://github.com/CyberTimon/RapidRAW/pull/1466).
The PR was discussed and its approach was not dismissed: it fixed the false
8-bit-in-a-16-bit-container output by adding a half-float (`rgba16float`)
render target and writing actual 16-bit TIFF samples. The limitation is that
half-float carries about 11 significant bits, not the full precision of a
16-bit integer render; near the brightest encoded stop its values fall on a
grid of roughly 32 out of 65,535 possible codes.

Argentum therefore built its own path: use an `rgba32float` output render
target, then quantise once to 16-bit integer samples when writing the TIFF.
This is a different implementation, not an unreviewed copy of PR #1466. It
removes the half-float **output-target** bottleneck, but is not end-to-end
32-bit: the input upload and some intermediate textures are still half-float,
so do not describe the whole export as full 16-bit precision or claim its
pixels are always more accurate without a direct comparison. The code's
precision limits and rationale are documented in
[`export_precision.rs`](../src-tauri/src/mods/export_precision.rs); the
implementation history and comparison are in [`ROADMAP.md`](ROADMAP.md),
under the #1466 entry.

**Catch-up instruction:** retain Argentum's implementation and tests while
reviewing RapidRAW's TIFF changes. Keep the work on the isolated catch-up
branch. Do not merge it into Argentum `main`, push it, or publish a PR before
the user has reviewed and explicitly approved that next step. Once the work
and tests are complete, send the result to SOL for the requested final review;
SOL's approval is a separate gate, not permission to merge without the user's
approval.

## 2026-09-13: crop update merged; highlight update temporarily deferred

**Decision:** merge the tested crop/cache update, but stop before the next
highlight/RAW-decoder commit until its interaction with Argentum is tested.
This is a temporary review boundary, not a rejection of upstream's approach.
The user explicitly approved the local merge and cleanup. Publication was not
authorized.

### What is already merged

- Consecutive RapidRAW history through
  `5ad3ba0b000186c6c2ce4637530c6cdbe94c7cad`: Ctrl-drag/Ctrl-wheel crop gestures,
  transformed-preview caching, the follow-up cache correction and cleanup.
- Upstream merge commit `a2159e42`; review/documentation commit `89d6f699`.
- Argentum fix `d94b4536`: do not mount the mask canvas when its width or height
  is zero. Hiding the editor on return to the library could otherwise produce
  a zero-sized canvas drawing error. The vulnerable code is inherited from
  RapidRAW, but the failure was not separately reproduced in untouched RapidRAW.
- Combined with the newer work on main at `3d2cc789`, without conflicts.

This used normal merges, not selective cherry-picks. Argentum's RAW decoder
revision and colour processing were not changed by this adopted batch.

### What remains pending, and why

Pending commit:
[`40cfa3df9c039f9f6adfd77a2c759b1ae788a3cd`](https://github.com/CyberTimon/RapidRAW/commit/40cfa3df9c039f9f6adfd77a2c759b1ae788a3cd),
“improve RAW highlight recovery and color clipping”.

It contains three coupled changes:

1. `src-tauri/src/raw_processing.rs` adds RGB highlight desaturation after
   demosaicing, including treatment of magenta highlights.
2. `src-tauri/src/image_processing.rs` stops cutting values above 1.0 after
   artifact removal and detail enhancement, while still bounding them below
   at zero.
3. `src-tauri/Cargo.lock` advances rawler from
   `3289454e9a65f5c973594687cca35602fa8181e3` to
   `934af4b213a12b95f77d69529627067056066294`, along with other lockfile changes.

Argentum already reconstructs clipped RAW channels before demosaicing. Adding
upstream's later RGB treatment could be useful, but could also change colour
or apply an additional correction to pixels we already recovered. Changing
the decoder and the handling of values above 1.0 also affects the input to
our processing. These are reasons to compare results, not evidence that
upstream's change is worse or incompatible.

The trial merge had no text conflicts. We deferred it because visual and
processing compatibility was unverified, not because Git could not merge it.
The crop/mask tests did not establish highlight quality. No roadmap feature
was cancelled or replaced by this decision.

**UI/UX review:** this exact commit changes only the three backend/dependency
files above; it adds no interface controls to inherit separately. For later
commits, still review the interface independently even if we retain our own
processing, as required by ARCHITECTURE.md.

### Evidence and limits of the completed testing

- User confirmed masks stayed aligned, exported crop matched, and returning
  from Mask/Crop appeared stable after the canvas guard.
- Thumbnail generation was initially reported slow, then appeared faster on
  retest without thumbnail-generation changes. Its cause was not isolated;
  do not describe this as a proven branch regression or a performance fix.
- Final combined source passed the frontend build, mergeability check,
  20 review regression checks and 7 integration checks. Existing warnings
  remained. Earlier candidate and baseline typechecks had the same 76 errors.
- Native compilation and a real RAW-to-JPEG export passed for the candidate.
  These do not replace visual highlight comparisons.
- Temporary executables, duplicate photos and native build cache were removed
  after approval. Rebuild from Git when resuming; do not depend on the old
  test-folder paths or claim those executables include later source changes.

### How the next session should catch up

1. Check current main, other active work and the latest upstream history.
   The hashes above record this decision, not a claim about today's latest tip.
   Use an isolated worktree and copied photos for comparison.
2. Start with pending `40cfa3df` as a whole. Inspect both the application diff
   and the rawler revision diff; do not treat a lockfile change as harmless
   bookkeeping. Include the current RAW/highlight and export-related registry
   entries in the review.
3. Compare current Argentum with the merged candidate using identical source
   RAW files, saved edits, backend, build profile and cache conditions. Include
   clipped coloured highlights, magenta edges, bright neutral areas and
   ordinary exposures. Compare editor results and exports. Include multiple
   cameras where samples are available. Judge retained detail and colour;
   compile success and numerical agreement alone are not quality evidence.
4. Decide explicitly whether to adopt upstream processing, keep ours or
   combine them. Record the evidence and both processing/UI conclusions in
   the review register. If keeping ours, reconcile that choice within the
   normal merge using our existing anchors and register the remaining
   difference. Retire redundant code and registry entries when appropriate.
5. Run the required mergeability checks and relevant image/export tests.
   Update this document with the resolution and commit hashes. Advance the
   reviewed-through marker only when that range has actually been reviewed.

**Sustainability constraint:** do not keep cherry-picking later updates around
this commit indefinitely. Resolve this boundary as part of the next catch-up
before declaring later upstream history adopted. A normal later merge will
include this ancestor; any retained Argentum behaviour needs an explicit,
reviewed reconciliation. Escalate a growing maintenance burden rather than
quietly accumulating drift. A new published release still needs the user's
explicit green light.

This is the repository record. The separate private project-brain decision log
was not updated in this session because its connector was unavailable; a
future session with access should cross-reference this decision there.
