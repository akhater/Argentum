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

/// What actually went wrong, not just where.
///
/// reqwest's own `Display` for a transport failure is "error sending request
/// for url (…)", which names the URL and says nothing about the cause — a DNS
/// failure, a refused connection, a proxy, an expired certificate and a
/// timeout all print the same sentence. The cause is one level down, in
/// `source()`, and hyper puts the useful part one level below that. So the
/// chain is walked and joined: "connection timed out" and "dns error" send a
/// person to different places, and the message they are shown should say which
/// one it was.
fn why(e: &dyn std::error::Error) -> String {
    let mut parts = vec![e.to_string()];
    let mut cause = e.source();
    while let Some(c) = cause {
        let text = c.to_string();
        if !parts.iter().any(|p| p == &text) {
            parts.push(text);
        }
        cause = c.source();
    }
    parts.join(": ")
}

/// One GET, over HTTP/2 if that works and HTTP/1.1 if it does not.
///
/// WHY THE SECOND ATTEMPT EXISTS
///
/// On the machine this was written on, every request to api.github.com from
/// reqwest failed after about ten seconds with rustls reporting "peer closed
/// connection without sending TLS close_notify", four times out of four, while
/// `curl` fetched the same URL in under half a second. The difference was not
/// the network and not the certificate: that build of curl cannot do HTTP/2 and
/// so never offers it, and reqwest offers it in the TLS handshake by default.
/// Something between this machine and GitHub accepts the connection, agrees to
/// HTTP/2 and then abandons it.
///
/// That is not ours to fix and not the user's to diagnose. It is also not rare:
/// HTTP/2 is what corporate inspection proxies and older firewalls mishandle
/// most, on every platform. HTTP/1.1 is understood by everything.
///
/// So the ordinary request is tried first, and a *transport* failure — not a
/// 404, not a rate limit, which are perfectly good responses — is retried once
/// on a client that cannot offer HTTP/2. Two small requests in the worst case,
/// for one that a person asked for and is waiting on.
///
/// Once HTTP/2 has failed here it is not tried again this session: the fault is
/// a property of the network, not of the request, and paying ten seconds for it
/// on every download would be worse than not offering HTTP/2 at all.
async fn get(url: &str, seconds: u64) -> Result<reqwest::Response, String> {
    /// Set the first time HTTP/2 fails, and never unset.
    static HTTP2_IS_BROKEN: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
    use std::sync::atomic::Ordering::Relaxed;

    fn client(http1_only: bool, seconds: u64) -> Result<reqwest::Client, String> {
        let builder = reqwest::Client::builder()
            .user_agent(AGENT)
            .timeout(std::time::Duration::from_secs(seconds));
        let builder = if http1_only { builder.http1_only() } else { builder };
        builder.build().map_err(|e| why(&e))
    }

    if HTTP2_IS_BROKEN.load(Relaxed) {
        return client(true, seconds)?.get(url).send().await.map_err(|e| why(&e));
    }

    let first = client(false, seconds)?.get(url).send().await;
    let Err(e) = first else {
        return first.map_err(|e| why(&e));
    };
    HTTP2_IS_BROKEN.store(true, Relaxed);

    client(true, seconds)?
        .get(url)
        .send()
        .await
        // The first failure is the one that describes the problem; the second
        // is only the confirmation that plain HTTP/1.1 could not save it.
        .map_err(|_| why(&e))
}

async fn listing() -> Result<Vec<Item>, String> {
    if let Ok(guard) = CACHE.lock()
        && let Some(items) = guard.as_ref()
    {
        return Ok(items
            .iter()
            .map(|i| Item { name: i.name.clone(), download_url: i.download_url.clone() })
            .collect());
    }

    let items: Vec<Item> = get(LISTING, 20)
        .await
        .map_err(|e| format!("could not reach RawTherapee's profile list — {e}"))?
        .json()
        .await
        .map_err(|e| format!("could not read the profile list — {}", why(&e)))?;

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

    let bytes = get(&found.url, 60)
        .await
        .map_err(|e| format!("download failed — {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("download failed — {}", why(&e)))?;

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

    /// A failure has to say what failed. This is the whole point of `why`:
    /// reqwest prints the URL and stops, so the chain below it is where the
    /// difference between "no network" and "certificate" lives.
    #[test]
    fn a_cause_is_carried_through_to_the_message() {
        #[derive(Debug)]
        struct Layer(&'static str, Option<Box<Layer>>);
        impl std::fmt::Display for Layer {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.0)
            }
        }
        impl std::error::Error for Layer {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                self.1.as_deref().map(|l| l as &(dyn std::error::Error + 'static))
            }
        }

        let e = Layer(
            "error sending request for url (https://api.github.com/...)",
            Some(Box::new(Layer("client error", Some(Box::new(Layer("connection timed out", None)))))),
        );
        let text = why(&e);
        assert!(text.contains("connection timed out"), "{text}");
        assert!(text.starts_with("error sending request"), "{text}");
    }

    /// The same cause repeated at two levels — which reqwest and hyper do —
    /// must not be printed twice.
    #[test]
    fn a_repeated_cause_is_said_once() {
        #[derive(Debug)]
        struct Same(Option<Box<Same>>);
        impl std::fmt::Display for Same {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("dns error")
            }
        }
        impl std::error::Error for Same {
            fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
                self.0.as_deref().map(|s| s as &(dyn std::error::Error + 'static))
            }
        }
        assert_eq!(why(&Same(Some(Box::new(Same(None))))), "dns error");
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
