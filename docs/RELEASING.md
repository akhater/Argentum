# Releasing

A release is a merge commit on `main`, published in this order.

1. **Version.** The tree already carries the next version, bumped right after
   the previous release ("Versioning" in [CHANGELOG.md](../CHANGELOG.md)). If the
   ISO week has rolled over since (`date +%g.%V`), the release sets the real
   `yy.ww.n` in `src-tauri/Cargo.toml`, Argentum's entry in `src-tauri/Cargo.lock`
   and `src-tauri/tauri.conf.json`.
2. **Notes.** First `node scripts/changelog.mjs` folds the entries waiting in
   `changelog.d/` into the version's section of `CHANGELOG.md`. That section, its plain-language line in
   `src/argentum/releases.ts`, and its rows in `docs/ROADMAP.md` and
   `src/argentum/roadmap.ts` say what actually shipped. Whatever a later PR made
   stale is fixed in a "Release <version>" PR.
3. **Speed check.** At the commit being released, on the laptop the budgets come
   from (named in `src-tauri/src/mods/speed_budgets.rs`), on mains power and with
   nothing else using the GPU, from `src-tauri/`:

   ```bash
   cargo test --lib mods::sharpen_gpu::tests::where_the_time_goes -- --ignored --nocapture --test-threads=1
   ```

   It ends with `speed: every row within its budget`, or fails with one line per
   row over budget: row, size, backend, measured and budget. **A breach blocks
   the release** until the slowdown is fixed. GitHub's runners have no real GPU,
   so this cannot run in CI.

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
4. **Publish.** `gh release create <version> --target <merge sha> --title
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
5. **Bump.** A "Bump development version to <next>" PR changes the same three
   lines as step 1.
