- **The macOS DMG opens again.** Since 26.45.2 the DMG carried the same self-signed signature as
  the app, and macOS refused to open it ("Apple could not verify ... is free of malware"). The app
  keeps its signature, so privacy answers still carry over between updates; the DMG is unsigned
  as before 26.45.2, and the release build fails if one is left signed (`scripts/tauri-macos.mjs`).
