# Releasing

A release is a merge commit on `main`, published in this order.

1. **Version.** The tree already carries the next version, bumped right after
   the previous release ("Versioning" in [CHANGELOG.md](../CHANGELOG.md)). If the
   ISO week has rolled over since (`date +%g.%V`), the release sets the real
   `yy.ww.n` in `src-tauri/Cargo.toml`, Argentum's entry in `src-tauri/Cargo.lock`
   and `src-tauri/tauri.conf.json`.
2. **Compatibility corpus.** With the real version set by step 1, from
   `src-tauri/`, write this release's folder and the snapshot of what the code
   renders for it:

   ```bash
   cargo test --lib mods::compat_fixture -- --ignored
   AG_COMPAT_UPDATE=1 cargo test --lib mods::compat_corpus
   ```

   Open `src-tauri/fixtures/compat/<version>/` and
   `src-tauri/fixtures/compat/snapshots/<version>.txt` and check that they hold
   only synthetic values (no path, name or value taken from a real photo or
   machine), and that the snapshot's diff from the previous release's is what
   the release changed. Commit both with the release. The folder of a version
   that has shipped is never written again. Then confirm nothing is missing:

   ```bash
   cargo test --lib mods::compat_corpus -- --include-ignored
   ```

   `the_release_being_cut_has_its_folder` in that run fails, naming what is
   absent, if the version in `Cargo.toml` has no folder, snapshot or
   processing-settings file. The release does not continue until it passes. (It is
   ignored in CI because the development version is always one ahead of the
   last folder.)
3. **Notes.** First `node scripts/changelog.mjs` folds the entries waiting in
   `changelog.d/` into the version's section of `CHANGELOG.md`. That section, its plain-language line in
   `src/argentum/releases.ts`, and its rows in `docs/ROADMAP.md` and
   `src/argentum/roadmap.ts` say what actually shipped. Whatever a later PR made
   stale is fixed in a "Release <version>" PR.
4. **Speed check.** At the commit being released, on the laptop the budgets come
   from (named in `src-tauri/src/mods/speed_budgets.rs`), on mains power and with
   nothing else using the GPU, from `src-tauri/`:

   ```bash
   cargo test --lib mods::sharpen_gpu::tests::where_the_time_goes -- --ignored --nocapture --test-threads=1
   ```

   It ends with `speed: every row within its budget`, or fails with one line per
   row over budget: row, size, backend, measured and budget. **A breach blocks
   the release** until the slowdown is fixed. GitHub's runners have no real GPU,
   so this cannot run in CI.

   The demosaic a new edit opens with, the same way:

   ```bash
   cargo test --lib mods::demosaic_measure::demosaic_within_budget -- --ignored --nocapture
   ```

   The Lensfun database parse start-up waits for, the same way:

   ```bash
   cargo test --profile measure --lib mods::speed_budgets::lens_parse_within_budget -- --ignored --nocapture
   ```

   The engine's slider tick, on the same laptop, from `src-tauri/engine/`, on one
   of your RAWs:

   ```bash
   AG_RAW=a.CR3 cargo test --test budgets -- --ignored --nocapture
   ```

   It prints each slider's ticks and fails when the median tick with every tool
   on is over its budget (`SLIDER_TICK` in `tests/budgets.rs`), or when no RAW is
   set. A breach blocks the release the same way.

   Then, on its own, **the golden photos on your own RAWs**: the check CI runs
   on ten public samples (`src-tauri/src/mods/look_check.rs`), on the RAWs you
   judge the app by, against references kept in `drafts.local/look`, never in
   the repository:

   ```bash
   AG_RAWS="a.CR3;b.NEF" AG_LOOK_REFS=../drafts.local/look \
     cargo test --lib mods::look_check::golden_photos -- --ignored --exact --nocapture
   ```

   A case that moved fails with what moved. If the release's notes say why,
   bless it: the same command with `AG_LOOK_BLESS="<why>"` and
   `AG_LOOK_CASES="<the cases>"`, which adds a row to
   `drafts.local/look/LEDGER.tsv`. If they do not, it blocks the release like a
   speed breach. The first run has no references: bless every case with
   `AG_LOOK_CASES="*"`.
5. **Publish.** `gh release create <version> --target <merge sha> --title
   "Argentum <version> — <summary>" --notes-file <notes>`, with no `v` in the
   tag. Creating the release starts `.github/workflows/release.yml`, which builds
   the installers and attaches them. Its last job then opens the notes with a
   **Download** table (Windows, Windows on ARM, Mac with Apple Silicon, Mac with
   Intel: a direct link to each file; Linux: which file to pick) and two
   lines: the Windows SmartScreen step and the macOS one-time steps
   (`scripts/release-downloads.mjs`). If a build failed, that job does not run;
   once the files are attached, run `node scripts/release-downloads.mjs
   <version>` by hand.
   Another job compares each installer with the same file of the previous
   release (`scripts/installer-size.mjs`) and fails when one grew by more than
   3%: add a line starting `Installer size:` to the notes saying why, then re-run
   that job.

   The macOS builds are signed with the self-signed certificate in the
   `MACOS_CERTIFICATE` secret, and fail without it. Keep that certificate:
   macOS ties each user's privacy answers to it, so a new one makes every Mac
   ask again once.
6. **Bump.** A "Bump development version to <next>" PR changes the same three
   lines as step 1.

## Periodic checks

- **Workflow action pins.** Every third-party action in `.github/workflows/` and
  `.github/actions/` is pinned to a full commit sha, with the tag it was taken
  from in a trailing comment. Nothing updates them on its own: review them at each
  release (`git grep -n "uses: .*@[0-9a-f]\{40\}"`) and move a pin by resolving
  the new tag with `gh api repos/<owner>/<repo>/git/ref/tags/<tag>`. The
  `tauri-action` pin is a commit from its `dev` branch; its FIXME says why.
