use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::Emitter;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalEditSession {
    pub source: String,
    pub output: String,
    pub format: String,
    pub jpeg_quality: u8,
}

#[derive(Clone, Debug)]
pub struct HeadlessExportSession {
    pub source: String,
    pub output: String,
    pub format: String,
    pub quality: u8,
    pub tiff_bit_depth: u8,
    pub keep_metadata: bool,
    pub adjustments_override: Option<String>,
}

#[derive(Clone, Debug)]
pub enum LaunchRequest {
    None,
    OpenFile(String),
    EditSession(ExternalEditSession),
    HeadlessExport(HeadlessExportSession),
    InvalidHeadless(String),
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LaunchPayload {
    pub open_with_file: Option<String>,
    pub edit_session: Option<ExternalEditSession>,
}

pub fn parse_launch_args(args: &[String]) -> LaunchRequest {
    if args.first().map(|s| s.as_str()) == Some("export") {
        let mut iter = args.iter().skip(1);

        let mut source = String::new();
        let mut output = String::new();
        let mut format = String::from("jpeg");
        let mut quality = 90;
        let mut tiff_bit_depth = 16;
        let mut keep_metadata = false;
        let mut adjustments_override = None;

        if let Some(src) = iter.next()
            && !src.starts_with('-')
        {
            source = src.clone();
        }

        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--output" => {
                    if let Some(out) = iter.next() {
                        output = out.clone();
                    }
                }
                "--format" => {
                    if let Some(fmt) = iter.next() {
                        format = fmt.clone();
                    }
                }
                "--quality" => {
                    if let Some(q) = iter.next() {
                        quality = q.parse().unwrap_or(90);
                    }
                }
                "--tiff-bit-depth" => {
                    let Some(value) = iter.next() else {
                        return LaunchRequest::InvalidHeadless(
                            "Missing value for --tiff-bit-depth; expected 8 or 16.".to_string(),
                        );
                    };
                    let Ok(value) = value.parse::<u8>() else {
                        return LaunchRequest::InvalidHeadless(format!(
                            "Invalid TIFF bit depth '{}'; expected 8 or 16.",
                            value
                        ));
                    };
                    tiff_bit_depth = match value {
                        8 | 16 => value,
                        _ => {
                            return LaunchRequest::InvalidHeadless(format!(
                                "Invalid TIFF bit depth '{}'; expected 8 or 16.",
                                value
                            ));
                        }
                    };
                }
                "--keep-metadata" => keep_metadata = true,
                "--adjustments" => {
                    if let Some(adj) = iter.next() {
                        adjustments_override = Some(adj.clone());
                    }
                }
                _ => {}
            }
        }

        return LaunchRequest::HeadlessExport(HeadlessExportSession {
            source,
            output,
            format,
            quality,
            tiff_bit_depth,
            keep_metadata,
            adjustments_override,
        });
    }

    let mut edit: Option<String> = None;
    let mut output: Option<String> = None;
    let mut format: Option<String> = None;
    let mut quality: Option<u8> = None;
    let mut plain: Option<String> = None;

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--edit" => edit = iter.next().cloned(),
            "--output" => output = iter.next().cloned(),
            "--format" => format = iter.next().cloned(),
            "--quality" => quality = iter.next().and_then(|q| q.parse().ok()),
            s if !s.starts_with('-') && plain.is_none() => plain = Some(s.to_string()),
            _ => {}
        }
    }

    match (edit, output) {
        (Some(source), Some(output)) => {
            let format = format.unwrap_or_else(|| {
                std::path::Path::new(&output)
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_lowercase())
                    .unwrap_or_else(|| "jpg".to_string())
            });
            let format = match format.as_str() {
                "tif" => "tiff".to_string(),
                _ => format,
            };
            LaunchRequest::EditSession(ExternalEditSession {
                source,
                output,
                format,
                jpeg_quality: quality.unwrap_or(90),
            })
        }
        (Some(source), None) => LaunchRequest::OpenFile(source),
        _ => match plain {
            Some(path) => LaunchRequest::OpenFile(path),
            None => LaunchRequest::None,
        },
    }
}

fn handle_file_open(app_handle: &tauri::AppHandle, path: PathBuf) {
    if let Some(path_str) = path.to_str()
        && let Err(e) = app_handle.emit("open-with-file", path_str)
    {
        log::error!("Failed to emit open-with-file event: {}", e);
    }
}

pub fn emit_launch_request(app_handle: &tauri::AppHandle, request: LaunchRequest) {
    match request {
        LaunchRequest::EditSession(session) => {
            if let Err(e) = app_handle.emit("external-edit-session", &session) {
                log::error!("Failed to emit external-edit-session event: {}", e);
            }
        }
        LaunchRequest::OpenFile(path) => {
            handle_file_open(app_handle, PathBuf::from(path));
        }
        LaunchRequest::HeadlessExport(_) => {
            println!(
                "Error: Headless export cannot be attached to an already running GUI instance."
            );
        }
        LaunchRequest::InvalidHeadless(error) => {
            log::error!("Invalid headless export request: {}", error);
        }
        LaunchRequest::None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{LaunchRequest, parse_launch_args};
    fn parse(args: &[&str]) -> LaunchRequest {
        parse_launch_args(
            &args
                .iter()
                .map(|arg| (*arg).to_string())
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn headless_tiff_depth_defaults_to_sixteen_bits() {
        let LaunchRequest::HeadlessExport(session) = parse(&["export", "photo.cr3"]) else {
            panic!("expected a headless export request");
        };
        assert_eq!(session.tiff_bit_depth, 16);
    }

    #[test]
    fn headless_tiff_depth_accepts_only_eight_or_sixteen_bits() {
        for value in ["8", "16"] {
            let LaunchRequest::HeadlessExport(session) =
                parse(&["export", "photo.cr3", "--tiff-bit-depth", value])
            else {
                panic!("expected a headless export request");
            };
            assert_eq!(session.tiff_bit_depth, value.parse::<u8>().unwrap());
        }
    }

    #[test]
    fn headless_tiff_depth_rejects_invalid_or_missing_values() {
        for args in [
            &["export", "photo.cr3", "--tiff-bit-depth"][..],
            &["export", "photo.cr3", "--tiff-bit-depth", "twelve"][..],
            &["export", "photo.cr3", "--tiff-bit-depth", "12"][..],
        ] {
            assert!(matches!(parse(args), LaunchRequest::InvalidHeadless(_)));
        }
    }
}
