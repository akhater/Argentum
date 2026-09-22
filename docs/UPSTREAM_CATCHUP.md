# RapidRAW catch-up decisions

This is the handoff for future upstream reviews. Read it alongside
`scripts/upstream-decisions.mjs` and the mergeability rules in
[ARCHITECTURE.md](ARCHITECTURE.md). A clean Git merge does not establish that
image processing still behaves correctly.

## 2026-09-19: split the highlight boundary by responsibility

**Decision:** adopt RapidRAW's scene-linear headroom change, keep Argentum's
pre-demosaic RAW recovery and explicit highlight-compression policy, and do
not stack RapidRAW's overlapping post-demosaic highlight recovery on top.

The adopted part is limited to `src-tauri/src/image_processing.rs`: the RAW
artifact-removal and detail-enhancement passes now floor intermediate RGB at
zero without an upper clamp at `1.0`. Values above nominal white therefore
survive until the later display/output mapping. This is deliberately a
separate change from RapidRAW's `raw_processing.rs` recovery algorithm.

Argentum's `mods::highlights::recover` remains the recovery authority. It runs
on the decoded CFA data before demosaicing and uses measured unclipped channel
ratios. RapidRAW's post-demosaic RGB correction is not imported because it
cannot distinguish a genuinely clipped pixel from a valid bright colour or a
pixel already reconstructed by Argentum. The upstream rawler lockfile change
also remains pending review rather than being pulled in as incidental
bookkeeping.

Numerical regression tests cover preservation of values above `1.0` and the
continued lower floor at zero. This resolves only the processing boundary;
the remaining upstream history still requires the normal feature-by-feature
review and mergeability checks.

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
