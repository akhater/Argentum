//! Fetching a camera profile from RawTherapee, on request.
//!
//! WHY THIS IS NOT THE SAME AS BUNDLING
//!
//! Argentum cannot ship profiles. RawTherapee's collection is bundled there
//! with each author's individual permission rather than under a licence that
//! lets anyone else redistribute it, so putting those files in our installer
//! would be assuming a right nobody granted.
//!
//! Downloading one is a different act. The file comes from the project that
//! published it, to the user's own machine, because the user asked — the same
//! thing a package manager does, and the same thing they would do by hand with
//! a browser. We are not the distributor; we are saving them the search.
//!
//! ON REQUEST, NEVER ON ITS OWN
//!
//! This runs when a button is clicked and at no other time. An app that reaches
//! out to a third party because a photo was opened is doing something the person
//! using it did not ask for and cannot see, and "it was convenient" is not a
//! good enough reason.
//!
//! IF IT BREAKS
//!
//! It depends on a directory listing in someone else's repository, which they
//! are free to rearrange. Every failure here is soft: the button reports that it
//! found nothing, and importing a file by hand still works exactly as before.

use std::sync::Mutex;

use super::profiles;

/// RawTherapee's published profiles.
///
/// The canonical path, not the old `Beep6581/RawTherapee` one: that repository
/// was renamed and now answers with a redirect. It happens to work because
/// reqwest follows redirects, which is a thing to depend on by accident rather
/// than on purpose.
///
/// One profile per camera, as of 161 files listed — so there is nothing to
/// choose between here, and "Find one" means what it says. Several profiles for
/// one body come from importing them.
const LISTING: &str =
    "https://api.github.com/repos/RawTherapee/RawTherapee/contents/rtdata/dcpprofiles?per_page=200";

/// GitHub refuses requests without one.
const AGENT: &str = "Argentum";

/// A profile that exists for this camera.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Found {
    pub file: String,
    pub url: String,
}

#[derive(serde::Deserialize)]
struct Item {
    name: String,
    download_url: Option<String>,
}

/// The listing, kept for the session.
///
/// It is a few hundred entries and does not change hour to hour; fetching it
/// once per camera the user asks about would be rude to a service that is doing
/// us a favour.
static CACHE: Mutex<Option<Vec<Item>>> = Mutex::new(None);

async fn listing() -> Result<Vec<Item>, String> {
    if let Ok(guard) = CACHE.lock()
        && let Some(items) = guard.as_ref()
    {
        return Ok(items
            .iter()
            .map(|i| Item { name: i.name.clone(), download_url: i.download_url.clone() })
            .collect());
    }

    let client = reqwest::Client::builder()
        .user_agent(AGENT)
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;

    let items: Vec<Item> = client
        .get(LISTING)
        .send()
        .await
        .map_err(|e| format!("could not reach RawTherapee's profile list: {e}"))?
        .json()
        .await
        .map_err(|e| format!("could not read the profile list: {e}"))?;

    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some(
            items
                .iter()
                .map(|i| Item { name: i.name.clone(), download_url: i.download_url.clone() })
                .collect(),
        );
    }
    Ok(items)
}

/// Is there a profile published for this body?
///
/// Matched with the same rule the library uses, on the file's stem, so a
/// near-miss like EOS 7D against EOS 7D Mark II cannot be offered.
pub async fn search(make: &str, model: &str) -> Result<Option<Found>, String> {
    let items = listing().await?;
    Ok(items.into_iter().find_map(|item| {
        let stem = item.name.strip_suffix(".dcp")?;
        profiles::matches(stem, make, model).then(|| Found {
            file: item.name.clone(),
            url: item.download_url.clone().unwrap_or_default(),
        })
    }))
}

/// Download one and put it in the library.
pub async fn fetch_into(library: &std::path::Path, found: &Found) -> Result<(), String> {
    if found.url.is_empty() {
        return Err("that profile has no download link".to_string());
    }

    let client = reqwest::Client::builder()
        .user_agent(AGENT)
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;

    let bytes = client
        .get(&found.url)
        .send()
        .await
        .map_err(|e| format!("download failed: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("download failed: {e}"))?;

    // Parsed before it is written, so a truncated or moved file is refused
    // rather than sitting in the library looking installed.
    super::dcp::parse(&bytes).map_err(|e| format!("what came back is not a profile: {e}"))?;

    std::fs::create_dir_all(library).map_err(|e| e.to_string())?;
    std::fs::write(library.join(&found.file), &bytes).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Matching is the library's rule, not a looser one: an offered profile for
    /// the wrong body is the same silent damage as an imported one.
    #[test]
    fn a_near_miss_is_not_offered() {
        // The real case: rawler reports the model alone, the profile is named
        // for the whole camera.
        assert!(profiles::matches("Canon EOS 5D Mark II", "Canon", "EOS 5D Mark II"));
        assert!(!profiles::matches("Canon EOS 7D", "Canon", "EOS 7D Mark II"));
    }

    /// A malformed entry must be refused before any network call.
    #[tokio::test]
    async fn a_download_without_a_link_is_refused() {
        let found = Found { file: "x.dcp".into(), url: String::new() };
        let err = fetch_into(std::path::Path::new("."), &found).await.unwrap_err();
        assert!(err.contains("no download link"), "{err}");
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;

    /// Against the real listing, because the matching has now been wrong twice
    /// in a way no offline test caught: both times the profile existed and the
    /// comparison could not see it. Only the actual filenames prove otherwise.
    #[tokio::test]
    #[ignore = "reaches the network; run by hand"]
    async fn it_finds_the_profile_for_a_real_camera() {
        let found = search("Canon", "EOS 5D Mark II")
            .await
            .expect("the listing should load");
        println!("with maker: {found:?}");
        assert!(found.is_some(), "no profile found with the maker known");

        // The state a gear list written before the maker was recorded is in.
        let blind = search("", "EOS 5D Mark II")
            .await
            .expect("the listing should load");
        println!("without maker: {blind:?}");
        assert!(blind.is_some(), "no profile found without the maker");
        assert_eq!(blind.unwrap().file, "Canon EOS 5D Mark II.dcp");
    }

    /// The whole path, not just the lookup: fetch it, parse it, and check the
    /// matrix is the shape a camera matrix should be. Downloading something
    /// that turns out not to be a profile is the failure this catches.
    #[tokio::test]
    #[ignore = "reaches the network; run by hand"]
    async fn it_downloads_and_the_file_is_a_real_profile() {
        let found = search("Canon", "EOS 5D Mark II")
            .await
            .expect("listing")
            .expect("a profile for this camera");

        let dir = std::env::temp_dir().join("argentum-online-download");
        let _ = std::fs::remove_dir_all(&dir);
        fetch_into(&dir, &found).await.expect("download");

        let bytes = std::fs::read(dir.join(&found.file)).expect("saved file");
        let profile = crate::mods::dcp::parse(&bytes).expect("parses");
        println!(
            "name {:?}  camera {:?}  illuminants {:?}/{:?}",
            profile.name, profile.camera, profile.illuminant1, profile.illuminant2
        );
        println!("colour matrix 1 {:?}", profile.colour_matrix1);
        const D50: [f32; 3] = [0.9642, 1.0, 0.8249];
        for row in 0..3 {
            let response: f32 = (0..3).map(|c| profile.colour_matrix1[row * 3 + c] * D50[c]).sum();
            assert!(response > 0.0, "row {row} responds {response} to white");
        }
        assert!(profile.forward_matrix1.is_some(), "no forward matrix");
    }
}
