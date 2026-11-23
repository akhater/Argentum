## Summary

<!-- What changes for someone using Argentum, in a sentence or two. -->

## Changes

<!-- The files or modules touched, and why. -->

## Testing

<!-- What was run, and on what. For a change to pixels: CIEDE2000 against the
     current render on the photos used, with the numbers. -->

## Not verified yet

<!-- Anything not checked, or "Nothing". -->

## Checklist

- [ ] A changelog entry as its own file, `changelog.d/<topic>.<added|changed|fixed|removed>.md` (never `CHANGELOG.md` itself), naming the source file and commit of anything taken from another project
- [ ] Credits updated in both `CREDITS.md` and `src/argentum/credits.ts` when a source is new or its scope changed
- [ ] Any new dependency (crate or npm package) has its reason above
- [ ] `npm run check:sources`, `npm run check:frontend`, `npm run check:wiring` and `npm run test:frontend` pass
- [ ] `npm run check:fmt` passes, `cargo test --lib` and `cargo clippy --all-targets -- -D warnings` pass in `src-tauri`, and `cargo test`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` pass in `src-tauri/engine` for engine changes
