//! Compare Argentum's colour against darktable's, over whole images.
//!
//! WHY
//!
//! AK sees a green cast that white balance cannot remove and that darktable does
//! not have. Hand-sampling a patch in both apps suggested green was ~13% high —
//! but he was right to distrust it: you cannot aim at the same pixel twice
//! across two applications, and his two attempts landed 30% apart in brightness.
//!
//! Ratios between channels held steady where absolute values did not, which is
//! suggestive but still two samples. This settles it over millions of pixels:
//! render the same file both ways at default settings and compare.
//!
//! Neither render is "right". The question is only whether they differ
//! *systematically* in one channel, which is what a colour-matrix difference
//! looks like and what no amount of white balance can fix.
//!
//! Produce darktable's side first:
//!
//! ```text
//! darktable-cli <raw> <out.tif> --width 1200 --height 1200 \
//!     --core --library ":memory:"
//! ```
//!
//! then run:
//!
//! ```text
//! cargo test --lib mods::colour_compare -- --ignored --nocapture
//! ```

#[cfg(test)]
mod tests_support {
    /// Shared by both comparison tests.
    pub fn brightness(a: &image::DynamicImage, b: &image::DynamicImage) -> (f64, f64) {
        let lum = |p: &image::Rgb<u8>| {
            0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64
        };
        let ra = a.to_rgb8();
        let rb = b.to_rgb8();
        (
            ra.pixels().map(lum).sum::<f64>() / ra.pixels().len() as f64,
            rb.pixels().map(lum).sum::<f64>() / rb.pixels().len() as f64,
        )
    }

    pub use super::tests::compare;
}

#[cfg(test)]
mod tests {
    use image::GenericImageView;

    const RAW: &str = r"C:\Users\you\OneDrive\Pictures\_Original to Review\2023\2023-06-01\2023-06-01_Canon EOS 5D Mark II_104-5729.CR2";
    const DARKTABLE: &str = r"C:\Users\you\NoCloudZone\Argentum\.compare\darktable_yourdefaults.tif";

    /// Compare the two renders pixel by pixel.
    ///
    /// The first version averaged each image separately over pixels above a
    /// brightness threshold - and the two images passed *different numbers of
    /// pixels* (611,504 against 527,665). Comparing those means is partly a
    /// measurement of which pixels survived the threshold in each, not of
    /// colour. Same pixel against same pixel, or the number means nothing.
    ///
    /// Ratios are taken per pixel and then averaged, so a bright area cannot
    /// dominate the answer.
    pub fn compare(a: &image::DynamicImage, b: &image::DynamicImage) -> (f64, f64, usize) {
        let ra = a.to_rgb8();
        let rb = b.to_rgb8();

        let (mut rg_a, mut rg_b, mut n) = (0f64, 0f64, 0usize);
        for (pa, pb) in ra.pixels().zip(rb.pixels()) {
            // Both must carry real colour, and green must be safely non-zero.
            if pa[1] < 40 || pb[1] < 40 {
                continue;
            }
            if pa[0].max(pa[1]).max(pa[2]) > 250 || pb[0].max(pb[1]).max(pb[2]) > 250 {
                continue; // clipped: ratios there are meaningless
            }
            rg_a += pa[0] as f64 / pa[1] as f64;
            rg_b += pb[0] as f64 / pb[1] as f64;
            n += 1;
        }
        let d = n.max(1) as f64;
        (rg_a / d, rg_b / d, n)
    }

    #[test]
    #[ignore = "needs a darktable render alongside; run by hand"]
    fn how_does_our_colour_differ_from_darktables() {
        let dt_path = std::path::Path::new(DARKTABLE);
        assert!(dt_path.exists(), "render darktable's side first: {DARKTABLE}");

        let dt = image::open(dt_path).expect("open darktable render");

        let bytes = std::fs::read(RAW).expect("read raw");
        let ours = crate::raw_processing::develop_raw_image(&bytes, false, 2.5, "off".to_string(), None)
            .expect("develop raw");

        // EXPERIMENT: how much of the gap is the white balance coefficients?
        // rawler uses the as-shot [2.179, 1.0, 1.623]; darktable reports
        // [2.574, 1.0, 1.388]. Scale our linear data by the ratio and see how
        // far it closes. Applied before the encode, since that is where
        // coefficients belong.
        let mut ours = ours;
        if std::env::var("AG_SIM_DT_WB").is_ok() {
            let (kr, kb) = (2.574 / 2.179, 1.388 / 1.623);
            let mut buf = ours.to_rgb32f();
            for p in buf.pixels_mut() {
                p[0] *= kr;
                p[2] *= kb;
            }
            ours = image::DynamicImage::ImageRgb32F(buf);
        }

        if std::env::var("AG_SIM_MATRIX").is_ok() {
            let c = super::matrix_sim::correction();
            let mut buf = ours.to_rgb32f();
            for p in buf.pixels_mut() {
                let (r, g, b) = (p[0], p[1], p[2]);
                p[0] = (c[0][0] * r + c[0][1] * g + c[0][2] * b).max(0.0);
                p[1] = (c[1][0] * r + c[1][1] * g + c[1][2] * b).max(0.0);
                p[2] = (c[2][0] * r + c[2][1] * g + c[2][2] * b).max(0.0);
            }
            ours = image::DynamicImage::ImageRgb32F(buf);
        }

        // Both sides must be display-referred or the comparison is meaningless:
        // darktable's export has its tone curve applied, and a tone curve
        // reshapes channel ratios. This is the preview encode the app applies
        // at default settings.
        crate::mods::preview_encode::apply(&mut ours);


        // NOTE highlight_compression 2.5, matching what the app passes from
        // settings. The first run used 1.0, which clamps every channel at 1.0 -
        // and red carries a 2.18x white balance multiplier, so red clipped
        // first and read 24% low. The measurement was of my own parameter.
        //
        // Match darktable's long edge so both are averaged over the same scene
        // content at the same scale.
        let (dw, dh) = dt.dimensions();
        let ours = ours.resize_exact(dw, dh, image::imageops::FilterType::Lanczos3);

        let (dt_rg, our_rg, n) = compare(&dt, &ours);

        println!("
{n} pixels compared, same position in both");
        {
            let da = dt.to_rgb8();
            let aa = ours.to_rgb8();
            let lum = |p: &image::Rgb<u8>| 0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64;
            let dl: f64 = da.pixels().map(lum).sum::<f64>() / da.pixels().len() as f64;
            let al: f64 = aa.pixels().map(lum).sum::<f64>() / aa.pixels().len() as f64;
            println!("
brightness  darktable {dl:.1}   argentum {al:.1}   ours is {:.0}% of theirs", al / dl * 100.0);
        }

        println!("darktable  mean R/G {dt_rg:.4}");
        println!("argentum   mean R/G {our_rg:.4}");
        println!(
            "difference {:+.1}%
",
            (our_rg / dt_rg - 1.0) * 100.0
        );
    }
}

/// Matrix experiments, kept out of the test body so it stays readable.
#[cfg(test)]
mod matrix_sim {
    /// Canon EOS 5D Mark II, from darktable's cameras.xml and rawler's log.
    pub const CANON_5D2: [[f32; 3]; 3] = [
        [0.4716, 0.0603, -0.0830],
        [-0.7798, 1.5474, 0.2480],
        [-0.1496, 0.1937, 0.6651],
    ];

    pub const SRGB_TO_XYZ: [[f32; 3]; 3] = [
        [0.412_456_4, 0.357_576_1, 0.180_437_5],
        [0.212_672_9, 0.715_152_2, 0.072_175_0],
        [0.019_333_9, 0.119_192_0, 0.950_304_1],
    ];

    pub const XYZ_TO_SRGB: [[f32; 3]; 3] = [
        [3.240_454_2, -1.537_138_5, -0.498_531_4],
        [-0.969_266_0, 1.876_010_8, 0.041_556_0],
        [0.055_643_4, -0.204_025_9, 1.057_225_2],
    ];

    /// Bradford, D50 -> D65. Adobe matrices are referenced to D50.
    pub const D50_TO_D65: [[f32; 3]; 3] = [
        [0.955_473_6, -0.023_098_5, 0.063_259_2],
        [-0.028_369_0, 1.009_995_1, 0.021_041_2],
        [0.012_314_2, -0.020_507_7, 1.330_889_5],
    ];

    pub fn mul(a: &[[f32; 3]; 3], b: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
        let mut o = [[0.0f32; 3]; 3];
        for (i, row) in o.iter_mut().enumerate() {
            for (j, c) in row.iter_mut().enumerate() {
                *c = (0..3).map(|k| a[i][k] * b[k][j]).sum();
            }
        }
        o
    }

    pub fn invert(m: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
        let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
        let d = 1.0 / det;
        [
            [
                (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * d,
                (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * d,
                (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * d,
            ],
            [
                (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * d,
                (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * d,
                (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * d,
            ],
            [
                (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * d,
                (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * d,
                (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * d,
            ],
        ]
    }

    pub fn row_normalise(m: &[[f32; 3]; 3]) -> [[f32; 3]; 3] {
        let mut o = *m;
        for row in o.iter_mut() {
            let s: f32 = row.iter().sum();
            for c in row.iter_mut() {
                *c /= s;
            }
        }
        o
    }

    /// What rawler computes: camera -> sRGB, from a row-normalised matrix.
    pub fn rawler_cam2rgb() -> [[f32; 3]; 3] {
        invert(&row_normalise(&mul(&CANON_5D2, &SRGB_TO_XYZ)))
    }

    /// The colorimetrically correct route: camera -> XYZ(D50) -> XYZ(D65) -> sRGB.
    pub fn correct_cam2rgb() -> [[f32; 3]; 3] {
        let cam_to_xyz_d50 = invert(&CANON_5D2);
        mul(&XYZ_TO_SRGB, &mul(&D50_TO_D65, &cam_to_xyz_d50))
    }

    /// Turn rawler's output into the correct one, without re-decoding.
    ///
    /// Row-normalised afterwards so neutral stays neutral: rawler's output
    /// already renders greys correctly and a fix that disturbs them is wrong,
    /// which is how the first attempt at this was caught.
    pub fn correction() -> [[f32; 3]; 3] {
        let c = mul(&correct_cam2rgb(), &invert(&rawler_cam2rgb()));
        row_normalise(&c)
    }
}

/// The same comparison across many photos, not one.
///
/// AK's objection, and a fair one: every number so far came from a single
/// frame, which is an easy way to tune a fix that only helps that frame. The
/// colour correction is derived from colour science rather than fitted, so it
/// should generalise by construction — this is what checks whether it does.
///
/// Renders each RAW through our pipeline and compares it against a darktable
/// render of the same file, produced beforehand with AK's own settings.
#[cfg(test)]
mod across_a_set {
    use super::tests_support::*;

    const DT_DIR: &str = r"C:\Users\you\NoCloudZone\Argentum\.compare\set";
    const RAW_ROOT: &str =
        r"C:\Users\you\OneDrive\Pictures\_Original to Review";

    fn find_raw(stem: &str) -> Option<std::path::PathBuf> {
        fn walk(dir: &std::path::Path, stem: &str) -> Option<std::path::PathBuf> {
            for e in std::fs::read_dir(dir).ok()?.flatten() {
                let p = e.path();
                if p.is_dir() {
                    if let Some(hit) = walk(&p, stem) {
                        return Some(hit);
                    }
                } else if p.file_stem().and_then(|s| s.to_str()).map(|s| {
                    s.chars()
                        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
                        .collect::<String>()
                }) == Some(stem.to_string())
                {
                    return Some(p);
                }
            }
            None
        }
        walk(std::path::Path::new(RAW_ROOT), stem)
    }

    #[test]
    #[ignore = "renders dozens of RAWs; run by hand"]
    fn colour_and_brightness_across_many_photos() {
        let dir = std::path::Path::new(DT_DIR);
        let mut rows = Vec::new();

        for entry in std::fs::read_dir(dir).expect("darktable set").flatten() {
            let dt_path = entry.path();
            if dt_path.extension().and_then(|e| e.to_str()) != Some("tif") {
                continue;
            }
            let stem = dt_path.file_stem().unwrap().to_string_lossy().to_string();
            let Some(raw) = find_raw(&stem) else {
                println!("no raw for {stem}");
                continue;
            };

            let Ok(bytes) = std::fs::read(&raw) else { continue };
            let Ok(mut ours) =
                crate::raw_processing::develop_raw_image(&bytes, false, 2.5, "off".to_string(), None)
            else {
                continue;
            };
            // The decoder applies exposure and the curve itself now.

            let dt = image::open(&dt_path).expect("open darktable render");
            let (dw, dh) = image::GenericImageView::dimensions(&dt);
            let ours = ours.resize_exact(dw, dh, image::imageops::FilterType::Lanczos3);

            let (dt_rg, our_rg, n) = compare(&dt, &ours);
            let (dl, al) = brightness(&dt, &ours);
            if n < 5000 {
                continue;
            }

            rows.push((stem, (our_rg / dt_rg - 1.0) * 100.0, al / dl * 100.0));
        }

        rows.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

        println!("\n{:<46} {:>10} {:>12}", "photo", "R/G gap", "brightness");
        for (name, rg, br) in &rows {
            println!("{:<46} {:>9.1}% {:>11.0}%", name, rg, br);
        }

        let n = rows.len() as f64;
        let mean_rg: f64 = rows.iter().map(|r| r.1).sum::<f64>() / n;
        let mean_br: f64 = rows.iter().map(|r| r.2).sum::<f64>() / n;
        let worst = rows.first().map(|r| r.1).unwrap_or(0.0);
        let best = rows.last().map(|r| r.1).unwrap_or(0.0);

        println!("\n{} photos", rows.len());
        println!("R/G gap     mean {mean_rg:.1}%   range {worst:.1}% to {best:.1}%");
        println!("brightness  mean {mean_br:.0}% of darktable\n");
    }
}

/// How the tones are spread, not just where their average sits.
///
/// AK's observation, looking at the in-app histogram: a spike jammed against
/// black and a separate bump in the mids, rather than a spread. That is a
/// distribution problem, and no exposure multiplier fixes it — brightening a
/// crushed image gives a brighter crushed image, which is precisely what the
/// +2.6 EV attempt produced.
///
/// Prints both histograms side by side so the shape can be compared rather than
/// argued about.
#[cfg(test)]
mod distribution {
    const DT: &str = r"C:\Users\you\NoCloudZone\Argentum\.compare\set\2023-06-25_Canon_EOS_5D_Mark_II_104-5897.tif";
    const RAW: &str = r"C:\Users\you\OneDrive\Pictures\_Original to Review\2023\2023-06-25\2023-06-25_Canon EOS 5D Mark II_104-5897.CR2";

    fn histogram(img: &image::DynamicImage) -> [f64; 16] {
        let rgb = img.to_rgb8();
        let mut bins = [0f64; 16];
        let mut total = 0f64;
        for p in rgb.pixels() {
            let luma = 0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64;
            let bin = ((luma / 256.0) * 16.0).floor().clamp(0.0, 15.0) as usize;
            bins[bin] += 1.0;
            total += 1.0;
        }
        for b in bins.iter_mut() {
            *b = *b / total * 100.0;
        }
        bins
    }

    fn bar(pct: f64) -> String {
        "#".repeat((pct * 1.2).round() as usize)
    }

    #[test]
    #[ignore = "reads AK's files; run by hand"]
    fn compare_tone_distribution() {
        let dt = image::open(DT).expect("darktable render");
        let bytes = std::fs::read(RAW).expect("raw");
        let mut ours =
            crate::raw_processing::develop_raw_image(&bytes, false, 2.5, "off".to_string(), None)
                .expect("develop");
        crate::mods::preview_encode::apply(&mut ours);

        let d = histogram(&dt);
        let a = histogram(&ours);

        println!("\n            darktable                      argentum");
        for i in 0..16 {
            println!(
                "{:>3}-{:<3} {:>5.1}% {:<24} {:>5.1}% {}",
                i * 16,
                (i + 1) * 16 - 1,
                d[i],
                bar(d[i]),
                a[i],
                bar(a[i])
            );
        }

        let shadows_d: f64 = d[0..3].iter().sum();
        let shadows_a: f64 = a[0..3].iter().sum();
        let mids_d: f64 = d[4..12].iter().sum();
        let mids_a: f64 = a[4..12].iter().sum();
        println!(
            "\nbottom 3 bins  darktable {shadows_d:.1}%   argentum {shadows_a:.1}%"
        );
        println!("midtones       darktable {mids_d:.1}%   argentum {mids_a:.1}%\n");
    }
}

/// Is the shadow crush a hard clip in the preview encode?
///
/// `apply_cpu_default_raw_processing` gamma-encodes, then applies
/// `(v - 0.5) * 1.28 + 0.5` and clamps to [0, 1]. That expression reaches zero
/// at v = 0.109, so every gamma-encoded value below that becomes exactly black —
/// scene-linear 0.109^2.38, about 0.57% brightness.
///
/// If that is the cause, a large share of pixels should land on precisely 0.0
/// after the encode, and the same pixels should be non-zero before it.
#[cfg(test)]
mod crush {
    const RAW: &str = r"C:\Users\you\OneDrive\Pictures\_Original to Review\2023\2023-06-25\2023-06-25_Canon EOS 5D Mark II_104-5897.CR2";

    #[test]
    #[ignore = "reads AK's file; run by hand"]
    fn how_much_is_clipped_to_black() {
        let raw_path = std::env::var("AG_RAW").unwrap_or_else(|_| RAW.to_string());
        println!("
file {raw_path}");
        let bytes = std::fs::read(&raw_path).expect("raw");
        let before =
            crate::raw_processing::develop_raw_image(&bytes, false, 2.5, "off".to_string(), None)
                .expect("develop");

        let mut after = before.clone();
        crate::mods::preview_encode::apply(&mut after);

        // Their original curve, for comparison: gamma then a contrast line that
        // crosses zero, clamped.
        let mut theirs = before.to_rgb32f();
        for p in theirs.pixels_mut() {
            for c in 0..3 {
                let g = p[c].max(0.0).powf(1.0 / 2.38);
                p[c] = ((g - 0.5) * 1.28 + 0.5).clamp(0.0, 1.0);
            }
        }
        let t = theirs;
        let mut zero_theirs = 0usize;

        let b = before.to_rgb32f();
        let a = after.to_rgb32f();

        let (mut zero_after, mut zero_before, mut total) = (0usize, 0usize, 0usize);
        let mut recoverable = 0usize;

        for ((pb, pa), pt) in b.pixels().zip(a.pixels()).zip(t.pixels()) {
            total += 1;
            if pt[0].max(pt[1]).max(pt[2]) <= 0.0 {
                zero_theirs += 1;
            }
            let dark_before = pb[0].max(pb[1]).max(pb[2]) <= 0.0;
            let dark_after = pa[0].max(pa[1]).max(pa[2]) <= 0.0;
            if dark_before {
                zero_before += 1;
            }
            if dark_after {
                zero_after += 1;
                if !dark_before {
                    recoverable += 1;
                }
            }
        }

        let pct = |n: usize| n as f64 / total as f64 * 100.0;
        println!("\npixels: {total}");
        println!("pure black BEFORE the encode: {:.1}%", pct(zero_before));
        println!("pure black AFTER  their encode: {:.1}%", pct(zero_theirs));
        println!("pure black AFTER  our encode:   {:.1}%", pct(zero_after));
        println!(
            "detail destroyed by the encode: {:.1}%   <- had values, came out black\n",
            pct(recoverable)
        );

        // The threshold the arithmetic predicts.
        let v: f32 = 0.5 - 0.5 / 1.28;
        println!("clips below gamma-encoded {v:.4}, i.e. scene-linear {:.5}", v.powf(2.38));
    }
}

/// Compare the two decoders at the linear stage, before any tone curve.
///
/// Everything earlier in this file compared finished renders, and a tone curve
/// reshapes channel ratios - so those numbers mixed "the colour is wrong" with
/// "the curve is different". This compares the decoders themselves.
///
/// darktable side: workflow `none` - as-shot white balance, standard camera
/// matrix, no tone module - exported as linear Rec709, which is linear with
/// sRGB primaries: the same thing `develop_raw_image` outputs. Made with:
///
/// ```text
/// darktable-cli <raw> .compare/dt_none_linrec709_16.tif --out-ext tif --hq true
///   --core --configdir <fresh dir>
///   --conf plugins/darkroom/workflow=none
///   --conf plugins/darkroom/chromatic-adaptation=legacy
///   --conf plugins/lighttable/export/icctype=3
///   --conf plugins/imageio/format/tiff/bpp=16
/// ```
#[cfg(test)]
mod linear_stage {
    use image::GenericImageView;

    const RAW: &str = r"C:\Users\you\OneDrive\Pictures\_Original to Review\2023\2023-06-01\2023-06-01_Canon EOS 5D Mark II_104-5729.CR2";
    const DT: &str = r"C:\Users\you\NoCloudZone\Argentum\.compare\dt_none_linrec709_16.tif";

    fn means(img: &image::DynamicImage) -> [f64; 3] {
        let rgb = img.to_rgb32f();
        let n = rgb.width() as f64 * rgb.height() as f64;
        let mut s = [0f64; 3];
        for p in rgb.pixels() {
            s[0] += p[0] as f64;
            s[1] += p[1] as f64;
            s[2] += p[2] as f64;
        }
        [s[0] / n, s[1] / n, s[2] / n]
    }

    /// Mean of per-pixel R/G and B/G, same position in both, on pixels that
    /// carry real signal and are clipped in neither.
    fn ratios(a: &image::DynamicImage, b: &image::DynamicImage) -> ([f64; 2], [f64; 2], usize) {
        let ra = a.to_rgb32f();
        let rb = b.to_rgb32f();
        let (mut ar, mut ab, mut br, mut bb, mut n) = (0f64, 0f64, 0f64, 0f64, 0usize);
        for (pa, pb) in ra.pixels().zip(rb.pixels()) {
            if pa[1] < 0.01 || pb[1] < 0.01 {
                continue;
            }
            if pa[0].max(pa[1]).max(pa[2]) > 0.9 || pb[0].max(pb[1]).max(pb[2]) > 0.9 {
                continue;
            }
            ar += (pa[0] / pa[1]) as f64;
            ab += (pa[2] / pa[1]) as f64;
            br += (pb[0] / pb[1]) as f64;
            bb += (pb[2] / pb[1]) as f64;
            n += 1;
        }
        let d = n.max(1) as f64;
        ([ar / d, ab / d], [br / d, bb / d], n)
    }

    fn srgb_to_linear(v: f32) -> f32 {
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    }

    fn develop(bytes: &[u8]) -> image::DynamicImage {
        crate::raw_processing::develop_raw_image(bytes, false, 2.5, "off".to_string(), None)
            .expect("develop raw")
    }

    #[test]
    #[ignore = "reads AK's file and a darktable render; run by hand"]
    fn where_is_the_cast_born() {
        // Point at any pair with AG_RAW / AG_DT; the constants are the default.
        let raw_path = std::env::var("AG_RAW").unwrap_or_else(|_| RAW.to_string());
        let dt_path = std::env::var("AG_DT").unwrap_or_else(|_| DT.to_string());
        println!("
raw: {raw_path}
dt:  {dt_path}");
        let dt = image::open(&dt_path).expect("render darktable's linear side first");
        let bytes = std::fs::read(&raw_path).expect("read raw");

        let ours = develop(&bytes);

        // The camera's own JPEG, linearised: a third opinion on the balance.
        // It carries the camera's picture style, so only a rough guide.
        let jpeg = rawler::analyze::extract_preview_pixels(&raw_path, &rawler::decoders::RawDecodeParams::default())
            .ok()
            .map(|j| {
                let mut f = j.to_rgb32f();
                for p in f.pixels_mut() {
                    for c in 0..3 {
                        p[c] = srgb_to_linear(p[c]);
                    }
                }
                image::DynamicImage::ImageRgb32F(f)
            });

        let (dw, dh) = dt.dimensions();
        println!(
            "\ndarktable {}x{}   ours {}x{}   jpeg {:?}",
            dw, dh, ours.width(), ours.height(),
            jpeg.as_ref().map(|j| j.dimensions())
        );

        let ours = ours.resize_exact(dw, dh, image::imageops::FilterType::Triangle);

        let md = means(&dt);
        let mo = means(&ours);
        let row = |name: &str, m: [f64; 3]| {
            println!("{:<26} {:>8.4} {:>8.4} {:>8.4}  {:>6.3}  {:>6.3}", name, m[0], m[1], m[2], m[0] / m[1], m[2] / m[1])
        };
        println!("\nlinear channel means              R        G        B     R/G     B/G");
        row("darktable (none)", md);
        row("argentum", mo);
        if let Some(j) = &jpeg {
            row("camera jpeg (linearised)", means(j));
        }

        println!(
            "\nours / darktable   R {:.3}  G {:.3}  B {:.3}",
            mo[0] / md[0], mo[1] / md[1], mo[2] / md[2]
        );

        let (d, o, n) = ratios(&dt, &ours);
        println!("\nper pixel, {n} positions        R/G      B/G");
        println!("darktable (none)          {:.4}   {:.4}", d[0], d[1]);
        println!(
            "argentum                  {:.4}   {:.4}   gap {:+.1}% / {:+.1}%\n",
            o[0], o[1], (o[0] / d[0] - 1.0) * 100.0, (o[1] / d[1] - 1.0) * 100.0
        );
    }
}

/// What rawler actually reads out of an sRAW, printed so it can be checked
/// against an independent parse of the same file.
#[cfg(test)]
mod sraw_facts {
    const SRAW: &str = r"C:\Users\you\OneDrive\Pictures\_Original to Review\2023\2023-06-01\2023-06-01_Canon EOS 5D Mark II_104-5729.CR2";

    #[test]
    #[ignore = "reads AK's file; run by hand"]
    fn what_rawler_reads_from_an_sraw() {
        let path = std::env::var("AG_SRAW").unwrap_or_else(|_| SRAW.to_string());
        let bytes = std::fs::read(&path).expect("read raw");
        let source = rawler::rawsource::RawSource::new_from_slice(&bytes);
        let decoder = rawler::get_decoder(&source).expect("decoder");
        let img = decoder
            .raw_image(&source, &rawler::decoders::RawDecodeParams::default(), false)
            .expect("decode");

        println!("\nfile        {path}");
        println!("camera      {} {}   mode {:?}", img.clean_make, img.clean_model, img.camera.mode);
        println!("size        {}x{}   cpp {}   photometric {:?}", img.width, img.height, img.cpp, img.photometric);
        println!("wb_coeffs   {:?}", img.wb_coeffs);
        println!("blacklevel  {:?}", img.blacklevel);
        println!("whitelevel  {:?}", img.whitelevel);
        println!("active_area {:?}", img.active_area);
        println!("crop_area   {:?}", img.crop_area);

        // Where the decoded data actually sits, per channel, before anything
        // touches it. If black is really zero the minimum tells us.
        let data = img.data.as_f32();
        let n = img.width * img.height;
        let mut min = [f32::MAX; 3];
        let mut max = [f32::MIN; 3];
        let mut sum = [0f64; 3];
        let mut zeros = [0usize; 3];
        for px in data.chunks_exact(img.cpp).take(n) {
            for c in 0..img.cpp.min(3) {
                let v = px[c];
                min[c] = min[c].min(v);
                max[c] = max[c].max(v);
                sum[c] += v as f64;
                if v <= 0.0 { zeros[c] += 1; }
            }
        }
        println!("\nraw data per channel (as decoded, before levels)");
        for c in 0..img.cpp.min(3) {
            println!(
                "  ch{c}  min {:>8.0}  max {:>8.0}  mean {:>9.1}  at-zero {:.2}%",
                min[c], max[c], sum[c] / n as f64, zeros[c] as f64 / n as f64 * 100.0
            );
        }
    }
}

/// The linear comparison over a spread of shoots, not one photo.
///
/// Same references as `linear_stage`, rendered for ten files across nine
/// different days and both RAW formats. This is the regression check: run it
/// after touching anything in the decode, and the numbers should not get worse.
///
/// It is what retired the D50/D65 matrix correction. That correction closed the
/// gap on the one frame it was built against, and once the sRAW levels bug was
/// fixed it was making nine of these ten worse — mean R/G error 6.8% with it
/// against 4.2% without. It had been compensating for the real bug.
#[cfg(test)]
mod linear_across_a_set {
    use image::GenericImageView;

    const DT_DIR: &str = r"C:\Users\you\NoCloudZone\Argentum\.compare\linset";
    const ROOT: &str = r"C:\Users\you\OneDrive\Pictures\_Original to Review";

    fn find_raw(stem: &str) -> Option<std::path::PathBuf> {
        let name = stem.replace('_', " ");
        let date = stem.get(..10)?;
        let year = stem.get(..4)?;
        let dir = std::path::Path::new(ROOT).join(year).join(date);
        for entry in std::fs::read_dir(dir).ok()? {
            let path = entry.ok()?.path();
            if path.extension().is_none_or(|e| !e.eq_ignore_ascii_case("CR2")) {
                continue;
            }
            let file = path.file_stem()?.to_string_lossy().to_string();
            if file == name || file.replace(' ', "_") == stem {
                return Some(path);
            }
        }
        None
    }

    /// Mean per-pixel R/G and B/G, same position in both images.
    fn ratios(a: &image::DynamicImage, b: &image::DynamicImage) -> ([f64; 2], [f64; 2], usize) {
        let ra = a.to_rgb32f();
        let rb = b.to_rgb32f();
        let (mut ar, mut ab, mut br, mut bb, mut n) = (0f64, 0f64, 0f64, 0f64, 0usize);
        for (pa, pb) in ra.pixels().zip(rb.pixels()) {
            if pa[1] < 0.01 || pb[1] < 0.01 {
                continue;
            }
            if pa[0].max(pa[1]).max(pa[2]) > 0.9 || pb[0].max(pb[1]).max(pb[2]) > 0.9 {
                continue;
            }
            ar += (pa[0] / pa[1]) as f64;
            ab += (pa[2] / pa[1]) as f64;
            br += (pb[0] / pb[1]) as f64;
            bb += (pb[2] / pb[1]) as f64;
            n += 1;
        }
        let d = n.max(1) as f64;
        ([ar / d, ab / d], [br / d, bb / d], n)
    }

    fn luma(img: &image::DynamicImage) -> f64 {
        let rgb = img.to_rgb32f();
        let n = rgb.width() as f64 * rgb.height() as f64;
        rgb.pixels()
            .map(|p| 0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64)
            .sum::<f64>()
            / n
    }

    #[test]
    #[ignore = "reads AK's files and darktable renders; run by hand"]
    fn how_close_are_we_across_the_set() {
        let mut rows: Vec<(String, bool, [f64; 3])> = Vec::new();

        for entry in std::fs::read_dir(DT_DIR).expect("render the linear set first") {
            let dt_path = entry.expect("dir entry").path();
            if dt_path.extension().is_none_or(|e| !e.eq_ignore_ascii_case("tif")) {
                continue;
            }
            let stem = dt_path.file_stem().unwrap().to_string_lossy().to_string();
            let Some(raw) = find_raw(&stem) else {
                println!("no raw for {stem}");
                continue;
            };
            let Ok(bytes) = std::fs::read(&raw) else { continue };

            let measure = || -> Option<[f64; 3]> {
                let ours = crate::raw_processing::develop_raw_image(
                    &bytes, false, 2.5, "off".to_string(), None,
                )
                .ok()?;
                let dt = image::open(&dt_path).ok()?;
                let (dw, dh) = dt.dimensions();
                let ours = ours.resize_exact(dw, dh, image::imageops::FilterType::Triangle);
                let (d, o, n) = ratios(&dt, &ours);
                if n < 5000 {
                    return None;
                }
                Some([
                    (o[0] / d[0] - 1.0) * 100.0,
                    (o[1] / d[1] - 1.0) * 100.0,
                    luma(&ours) / luma(&dt) * 100.0,
                ])
            };

            let Some(gap) = measure() else { continue };

            // sRAW decodes to three channels; full RAW to one and then demosaics.
            let source = rawler::rawsource::RawSource::new_from_slice(&bytes);
            let sraw = rawler::get_decoder(&source)
                .and_then(|d| d.raw_image(&source, &rawler::decoders::RawDecodeParams::default(), false))
                .map(|i| i.cpp == 3)
                .unwrap_or(false);

            rows.push((stem, sraw, gap));
        }

        rows.sort_by(|a, b| a.0.cmp(&b.0));
        println!("\n{:<42} {:>5} {:>9} {:>9} {:>12}", "photo", "kind", "R/G", "B/G", "brightness");
        for (name, sraw, gap) in &rows {
            println!(
                "{:<42} {:>5} {:>+8.1}% {:>+8.1}% {:>11.0}%",
                name.get(..42).unwrap_or(name),
                if *sraw { "sRAW" } else { "full" },
                gap[0], gap[1], gap[2]
            );
        }

        let n = rows.len().max(1) as f64;
        let mag = |i: usize| rows.iter().map(|r| r.2[i].abs()).sum::<f64>() / n;
        let bright = rows.iter().map(|r| r.2[2]).sum::<f64>() / n;
        println!(
            "\n{} photos   mean |R/G| off {:.1}%   mean |B/G| off {:.1}%   brightness {:.0}% of darktable\n",
            rows.len(), mag(0), mag(1), bright
        );
    }
}
