- **Headless export can size and pick its photos, and reports each one.** `argentum export`
  takes `--long-edge <px>` to write each photo at that long edge (never enlarged) instead of full
  size; `--edited-only` to export only a folder's photos that carry an edit, the ones the thumbnails
  mark as edited; and `--files <list.txt>` in place of a source, one path per line, where a virtual
  copy is given as `<photo path>?vc=<copy id>`. Every photo asked for gets one JSON line on stdout
  (`source`, `copy`, `output` and `ok`, or `error`), and the progress messages moved to stderr. The
  exit code is non-zero when any photo failed; a folder with no edited photo exits without an error.
