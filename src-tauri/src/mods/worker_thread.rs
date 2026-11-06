//! A command's work on a worker thread, off the UI thread. Ours.
//!
//! A synchronous `#[tauri::command]` runs on the UI thread (`ui_thread.rs`), so
//! the window waits for whatever it waits for: a folder listed on a network
//! share that dropped, a NAS gone to sleep or a USB disk that stopped answering
//! freezes the whole window until Windows gives up on the drive. An `async`
//! command that hands its work to [`on_worker_thread`] waits there instead, and
//! the window carries on.
//!
//! Adapted from Shoebox, src-tauri/src/commands.rs @ 5a60d6f (`blocking`), with
//! the error text of Argentum's own `spawn_blocking` calls.

/// Runs `work` on a worker thread and waits for it there: the thread that
/// awaits this is free meanwhile.
pub async fn on_worker_thread<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| format!("Task failed: {}", e))?
}

#[cfg(test)]
mod tests {
    use super::on_worker_thread;
    use crate::app_settings::AppSettings;
    use crate::file_management::{EnqueueMetadata, ImageFile, images_in_dir, images_recursive};
    use std::fs;
    use std::path::Path;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    use tauri::async_runtime::block_on;

    type Listing = fn(&str, &AppSettings, EnqueueMetadata) -> Result<Vec<ImageFile>, String>;

    const NONE_ONLINE_ONLY: EnqueueMetadata =
        &|path, _, _| panic!("{path}: nothing in the fixture is online-only");

    fn write(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn edit_file(rating: u8) -> Vec<u8> {
        format!(r#"{{"version":1,"rating":{rating},"adjustments":{{}}}}"#).into_bytes()
    }

    /// A folder that cannot be listed, until [`readable_again`].
    #[cfg(windows)]
    fn make_unreadable(folder: &Path) {
        let status = std::process::Command::new("icacls")
            .arg(folder)
            .args(["/deny", "*S-1-1-0:(RD)"])
            .output()
            .unwrap()
            .status;
        assert!(status.success(), "icacls /deny");
    }

    #[cfg(windows)]
    fn readable_again(folder: &Path) {
        let _ = std::process::Command::new("icacls")
            .arg(folder)
            .args(["/remove:d", "*S-1-1-0"])
            .output();
    }

    #[cfg(unix)]
    fn make_unreadable(folder: &Path) {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(folder, fs::Permissions::from_mode(0o000)).unwrap();
    }

    #[cfg(unix)]
    fn readable_again(folder: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(folder, fs::Permissions::from_mode(0o755));
    }

    /// Everything a folder of photos holds that a listing treats apart.
    struct Fixture {
        root: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let at = |name: &str| root.path().join(name);
            // A RAW and its camera JPEG (one group), its edit file and a virtual copy.
            write(&at("IMG_0001.CR3"), b"raw");
            write(&at("IMG_0001.JPG"), b"jpeg");
            write(&at("IMG_0001.CR3.agdata"), &edit_file(3));
            write(&at("IMG_0001.CR3.a1b2c3.agdata"), &edit_file(5));
            // A look file, an edit file whose photo is gone, a file that is no photo.
            write(&at("IMG_0001.CR3.cam1.abc.lut.png"), b"look");
            write(&at("gone.CR3.agdata"), &edit_file(1));
            write(&at("notes.txt"), b"text");
            // Hidden: by its name, and by its attribute on Windows.
            write(&at(".hidden.jpg"), b"jpeg");
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .attributes(FILE_ATTRIBUTE_HIDDEN)
                    .open(at("attr_hidden.NEF"))
                    .unwrap();
            }
            // Folders below, one of them with an edited photo, one a level deeper.
            write(&at("day2/DSC_0002.NEF"), b"raw");
            write(&at("day2/DSC_0002.NEF.agdata"), &edit_file(4));
            write(&at("day2/deeper/P0003.ORF"), b"raw");
            write(&at("day2/deeper/P0003.jpg"), b"jpeg");
            // A folder that cannot be read.
            write(&at("locked/L0004.CR3"), b"raw");
            make_unreadable(&at("locked"));
            Self { root }
        }

        fn path(&self, below: &str) -> String {
            self.root.path().join(below).to_string_lossy().into_owned()
        }

        /// Paths as the listing names them, relative to the fixture, `/` between folders.
        fn relative(&self, files: &[ImageFile]) -> Vec<String> {
            let root = format!(
                "{}{}",
                self.root.path().display(),
                std::path::MAIN_SEPARATOR
            );
            let mut names: Vec<String> = files
                .iter()
                .map(|f| f.path.trim_start_matches(&root).replace('\\', "/"))
                .collect();
            names.sort();
            names
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            readable_again(&self.root.path().join("locked"));
        }
    }

    /// What the frontend gets, byte for byte: the listing, or its error, as JSON.
    fn answer(listing: &Result<Vec<ImageFile>, String>) -> String {
        serde_json::to_string(listing).unwrap()
    }

    fn on_the_caller(listing: Listing, folder: &str) -> Result<Vec<ImageFile>, String> {
        listing(folder, &AppSettings::default(), NONE_ONLINE_ONLY)
    }

    fn on_a_worker(listing: Listing, folder: &str) -> Result<Vec<ImageFile>, String> {
        let folder = folder.to_string();
        block_on(on_worker_thread(move || {
            listing(&folder, &AppSettings::default(), NONE_ONLINE_ONLY)
        }))
    }

    #[test]
    fn a_listing_on_a_worker_thread_answers_exactly_as_on_the_caller() {
        let fixture = Fixture::new();
        let listings: [(&str, Listing); 2] =
            [("in_dir", images_in_dir), ("recursive", images_recursive)];
        for (name, listing) in listings {
            for below in ["", "day2", "day2/deeper", "locked", "missing"] {
                let folder = fixture.path(below);
                let before = answer(&on_the_caller(listing, &folder));
                let after = answer(&on_a_worker(listing, &folder));
                assert_eq!(before, after, "{name} of '{below}'");
            }
        }
    }

    /// The fixture is what the test above says it is: each case is in it.
    #[test]
    fn the_fixture_holds_every_case() {
        let fixture = Fixture::new();
        // A name starting with a dot is left out; the hidden attribute is not.
        let mut in_dir = vec!["IMG_0001.CR3", "IMG_0001.CR3?vc=a1b2c3", "IMG_0001.JPG"];
        if cfg!(windows) {
            in_dir.push("attr_hidden.NEF");
        }
        in_dir.sort();
        let listed = on_the_caller(images_in_dir, &fixture.path("")).unwrap();
        assert_eq!(fixture.relative(&listed), in_dir);

        let photo = |path: &str| listed.iter().find(|f| f.path.ends_with(path)).unwrap();
        let json = |path: &str| serde_json::to_value(photo(path)).unwrap();
        assert_eq!(json("IMG_0001.CR3")["rating"], 3);
        assert_eq!(json("?vc=a1b2c3")["rating"], 5);
        assert_eq!(json("?vc=a1b2c3")["is_virtual_copy"], true);
        assert!(json("IMG_0001.CR3")["group_id"].is_string());
        assert_eq!(
            json("IMG_0001.CR3")["group_id"],
            json("IMG_0001.JPG")["group_id"]
        );

        let mut recursive = in_dir.clone();
        recursive.extend([
            "day2/DSC_0002.NEF",
            "day2/deeper/P0003.ORF",
            "day2/deeper/P0003.jpg",
        ]);
        recursive.sort();
        let walked = on_the_caller(images_recursive, &fixture.path("")).unwrap();
        assert_eq!(
            fixture.relative(&walked),
            recursive,
            "the locked folder is left out"
        );

        assert!(on_the_caller(images_in_dir, &fixture.path("locked")).is_err());
        assert!(on_the_caller(images_in_dir, &fixture.path("missing")).is_err());
    }

    /// A drive that does not answer keeps only the listing waiting: another
    /// command, run meanwhile on the thread that asked, goes to its end.
    #[test]
    fn a_listing_that_waits_leaves_its_caller_free() {
        let fixture = Fixture::new();
        let folder = fixture.path("");
        let expected = answer(&on_the_caller(images_in_dir, &folder));
        let (answer_drive, drive) = mpsc::channel::<()>();
        let (listed, listing_done) = mpsc::channel();
        let listing = on_worker_thread(move || {
            drive.recv().map_err(|e| e.to_string())?;
            images_in_dir(&folder, &AppSettings::default(), NONE_ONLINE_ONLY)
        });
        tauri::async_runtime::spawn(async move {
            let _ = listed.send(listing.await);
        });

        let other = block_on(on_worker_thread(|| Ok("another command")));
        assert_eq!(other, Ok("another command"));
        assert!(
            listing_done.try_recv().is_err(),
            "the listing is still waiting on the drive"
        );

        answer_drive.send(()).unwrap();
        let listing = listing_done.recv_timeout(Duration::from_secs(30)).unwrap();
        assert_eq!(answer(&listing), expected);
    }

    /// Not slower: the same large folder listed on the caller (as the command did
    /// before) and through a worker thread, the median of five each. Run with
    /// `cargo test --lib worker_thread -- --ignored --nocapture`.
    #[test]
    #[ignore = "a measurement"]
    fn measure_a_large_folder() {
        let root = tempfile::tempdir().unwrap();
        for n in 0..3000 {
            write(&root.path().join(format!("IMG_{n:04}.CR3")), b"raw");
            write(&root.path().join(format!("IMG_{n:04}.JPG")), b"jpeg");
            if n % 10 == 0 {
                write(
                    &root.path().join(format!("IMG_{n:04}.CR3.agdata")),
                    &edit_file(2),
                );
            }
        }
        let folder = root.path().to_string_lossy().into_owned();
        let count = |files: Result<Vec<ImageFile>, String>| {
            assert_eq!(files.unwrap().len(), 6000);
        };
        let time = |run: &dyn Fn()| {
            let start = Instant::now();
            run();
            start.elapsed()
        };
        let median = |mut times: Vec<Duration>| {
            times.sort();
            times[times.len() / 2]
        };
        for (name, listing) in [
            ("in_dir", images_in_dir as Listing),
            ("recursive", images_recursive),
        ] {
            // Once first, so no run pays for a cold disk cache; then turn about, so
            // neither side gets the quieter moments.
            count(on_the_caller(listing, &folder));
            let (mut caller, mut worker) = (Vec::new(), Vec::new());
            for _ in 0..5 {
                caller.push(time(&|| count(on_the_caller(listing, &folder))));
                worker.push(time(&|| count(on_a_worker(listing, &folder))));
            }
            println!(
                "6000 photos, {name}, median of 5: on the caller {:?}, on a worker {:?}",
                median(caller),
                median(worker)
            );
        }
        // What the worker thread itself adds: handing over work that does nothing.
        let hand_over = median(
            (0..101)
                .map(|_| time(&|| block_on(on_worker_thread(|| Ok(()))).unwrap()))
                .collect(),
        );
        println!("handing work to a worker thread, median of 101: {hand_over:?}");
    }
}
