- **Linear DNGs from phones such as the Galaxy S23 Ultra no longer open completely white.** These
  files give a black level for each colour of a repeating 2 x 2 pattern (12 values) but one white
  level per colour (3 values). rawler only scaled the picture when the two counts matched, so it
  skipped the scaling and every pixel came out far above white, past what Exposure can bring back.
  The black level pattern is now applied and 1.0 is the white level, in Argentum's rawler fork
  (`akhater/RapidRAW-DngLab` @ `f90d2acc`), following dnglab's black level model
  (`rawler/src/imgop/blacklevel.rs` @ `0f645aa6`). JPEG XL compressed DNGs with 9 to 15 bits per
  sample no longer stop the decode either, with dnglab's fix
  (`rawler/src/decompressors/jpegxl.rs` @ `49445353`, fork @ `dc8b144a`). Thumbnails are made again once.
