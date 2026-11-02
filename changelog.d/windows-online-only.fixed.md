- **Opening a folder on Windows no longer downloads its online-only photos.** The check for files
  that are not on this computer existed only for macOS, so on Windows a OneDrive folder fetched every
  photo for its thumbnails and metadata. Online-only photos now show a cloud icon instead of a
  thumbnail until they are downloaded, and opening one fetches it as before. Culling and AI tagging
  skip them. The read-ahead on hard disks, the embedded XMP scan and AI tagging now leave online-only
  photos unread on macOS too. Small sidecar files that are online-only are still fetched, in the
  background, for ratings and edits. The tooltip says "the cloud" rather than iCloud.
