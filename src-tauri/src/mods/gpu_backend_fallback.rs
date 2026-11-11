//! Which graphics backend to try when the first one refuses a device. Ours,
//! after RAWmakase.
//!
//! wgpu commits to one adapter and gives up when it cannot have a device, so a
//! driver that refused one backend left the app with no GPU at all, even where
//! another backend would have worked. Now each backend the first try did not
//! cover is tried in turn, each on an instance of its own, in the same launch.
//!
//! The first try is untouched: a computer that starts today takes the same
//! path. Only an `Err` from `request_adapter` or `request_device` gets here. A
//! crash during setup is the setup mark's job (`gpu_setup_mark.rs`).
//!
//! A refused adapter rules out every backend its instance held, since asking
//! one of them again on its own would get the same answer. A refused device
//! rules out only the adapter's own backend.

// RAWmakase 02c5335d: crates/rawmakase-engine/src/develop/gpu/mod.rs (MIT) -
// DirectX 12, then Vulkan, then OpenGL on Windows, and the failing call named
// in the error.

use wgpu::Backends;

/// The backends to fall back on, best first, for this platform.
#[cfg(target_os = "windows")]
pub const ORDER: &[Backends] = &[Backends::DX12, Backends::VULKAN, Backends::GL];
#[cfg(target_os = "macos")]
pub const ORDER: &[Backends] = &[Backends::METAL, Backends::GL];
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub const ORDER: &[Backends] = &[Backends::VULKAN, Backends::GL];

/// Why a try did not produce a device. `backends` are the ones it rules out.
#[derive(Debug)]
pub struct Refusal {
    pub backends: Backends,
    pub error: String,
}

/// The backends in `order` that `tried` did not cover, in that order.
pub fn next_backends(order: &[Backends], tried: Backends) -> Vec<Backends> {
    order
        .iter()
        .copied()
        .filter(|backend| !tried.intersects(*backend))
        .collect()
}

/// `first` when it worked; otherwise each backend `next_backends` leaves,
/// opened by `open`, until one works. The error names every backend tried and
/// why it failed.
pub fn open_with_fallback<T>(
    first: Result<T, Refusal>,
    order: &[Backends],
    mut open: impl FnMut(Backends) -> Result<T, Refusal>,
) -> Result<T, String> {
    let first = match first {
        Ok(opened) => return Ok(opened),
        Err(refusal) => refusal,
    };
    let next = next_backends(order, first.backends);
    log::warn!(
        "GPU setup failed on {}: {}. Trying {}.",
        name(first.backends),
        first.error,
        if next.is_empty() {
            "nothing else".to_string()
        } else {
            next.iter()
                .map(|b| name(*b))
                .collect::<Vec<_>>()
                .join(", then ")
        }
    );

    let mut refusals = vec![first];
    for backends in next {
        match open(backends) {
            Ok(opened) => {
                log::warn!("GPU setup fell back to {}.", name(backends));
                return Ok(opened);
            }
            Err(refusal) => {
                log::warn!(
                    "GPU setup failed on {}: {}",
                    name(refusal.backends),
                    refusal.error
                );
                refusals.push(refusal);
            }
        }
    }
    Err(format!(
        "No graphics backend could start: {}",
        refusals
            .iter()
            .map(|r| format!("{}: {}", name(r.backends), r.error))
            .collect::<Vec<_>>()
            .join("; ")
    ))
}

/// `DX12`, or `VULKAN+DX12` for an instance that held more than one.
fn name(backends: Backends) -> String {
    backends
        .iter_names()
        .map(|(name, _)| name)
        .collect::<Vec<_>>()
        .join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOWS: &[Backends] = &[Backends::DX12, Backends::VULKAN, Backends::GL];

    fn refused(backends: Backends, error: &str) -> Refusal {
        Refusal {
            backends,
            error: error.to_string(),
        }
    }

    #[test]
    fn a_refused_device_moves_on_to_the_other_backends_in_order() {
        assert_eq!(
            next_backends(WINDOWS, Backends::DX12),
            [Backends::VULKAN, Backends::GL]
        );
        assert_eq!(
            next_backends(WINDOWS, Backends::VULKAN),
            [Backends::DX12, Backends::GL]
        );
    }

    #[test]
    fn a_refused_adapter_rules_out_its_whole_instance() {
        // Windows' first instance holds PRIMARY: only OpenGL is left.
        assert_eq!(next_backends(WINDOWS, Backends::PRIMARY), [Backends::GL]);
        assert!(next_backends(WINDOWS, Backends::all()).is_empty());
    }

    #[test]
    fn a_backend_chosen_by_wgpu_backend_still_falls_back() {
        // The one OpenGL launch after an unfinished setup sets WGPU_BACKEND=gl.
        assert_eq!(
            next_backends(WINDOWS, Backends::GL),
            [Backends::DX12, Backends::VULKAN]
        );
    }

    #[test]
    fn every_platform_falls_back_on_opengl_last() {
        assert_eq!(ORDER.last(), Some(&Backends::GL));
        assert!(ORDER.iter().all(|b| b.bits().count_ones() == 1));
    }

    #[test]
    fn a_working_first_try_tries_nothing_else() {
        let opened = open_with_fallback(Ok("first"), WINDOWS, |_| panic!("tried a backend"));
        assert_eq!(opened, Ok("first"));
    }

    #[test]
    fn the_first_backend_that_opens_is_kept() {
        let mut tried = Vec::new();
        let opened = open_with_fallback(
            Err(refused(Backends::DX12, "request_device: E_INVALIDARG")),
            WINDOWS,
            |backends| {
                tried.push(backends);
                if backends == Backends::VULKAN {
                    Err(refused(backends, "request_adapter: none"))
                } else {
                    Ok(backends)
                }
            },
        );
        assert_eq!(opened, Ok(Backends::GL));
        assert_eq!(tried, [Backends::VULKAN, Backends::GL]);
    }

    #[test]
    fn the_final_error_lists_each_backend_and_why() {
        let error = open_with_fallback::<()>(
            Err(refused(Backends::DX12, "request_device: lost")),
            WINDOWS,
            |backends| Err(refused(backends, "request_adapter: none")),
        )
        .unwrap_err();
        assert_eq!(
            error,
            "No graphics backend could start: DX12: request_device: lost; \
             VULKAN: request_adapter: none; GL: request_adapter: none"
        );
    }
}
