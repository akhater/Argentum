- **A Mac remembers your folder permissions across updates.** The macOS builds are now signed with
  one self-signed certificate, so an update is no longer a new app to macOS. The first signed
  release asks once more. The prompts for Desktop, Documents, Downloads, external drives and
  network drives now say why Argentum asks (`src-tauri/Info.plist`, merged by Tauri). Installing
  still needs the one-time `xattr` step, because the builds are not notarized.
