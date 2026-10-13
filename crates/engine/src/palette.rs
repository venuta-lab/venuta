//! The colours of a cover: the one that is most the cover's (its seed), and a second one far
//! enough from it in hue, when the cover has one. Research: docs/research/07-colour-from-artwork.md.
//!
//! Averaging the most saturated pixels (what came before) mixed different hues into mud and
//! was won by small, loud things: the label badge in a film cover's corner. Instead, after
//! Material's content-colour scoring: pixels are grouped by hue in OKLCH; a hue scores by how
//! much of the cover it (and its neighbours within 15 degrees) covers and by how colourful it
//! is; the corners and edges count less than the middle, where the picture is.

use image::RgbImage;

/// A colour in OKLCH (lightness 0-1, chroma 0-~0.37, hue in degrees).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lch {
    pub l: f32,
    pub c: f32,
    pub h: f32,
}

fn to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn to_gamma(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// sRGB (0-255) to OKLab.
pub fn oklab([r, g, b]: [u8; 3]) -> [f32; 3] {
    let (r, g, b) = (
        to_linear(r as f32 / 255.0),
        to_linear(g as f32 / 255.0),
        to_linear(b as f32 / 255.0),
    );
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

pub fn lch_of(lab: [f32; 3]) -> Lch {
    let c = (lab[1] * lab[1] + lab[2] * lab[2]).sqrt();
    let h = lab[2].atan2(lab[1]).to_degrees().rem_euclid(360.0);
    Lch { l: lab[0], c, h }
}

/// OKLCH to `#rrggbb`, lowering chroma (hue and lightness kept) until it fits sRGB.
pub fn hex_of(lch: Lch) -> String {
    let mut c = lch.c;
    loop {
        let (a, b) = (c * lch.h.to_radians().cos(), c * lch.h.to_radians().sin());
        let l_ = (lch.l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
        let m_ = (lch.l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
        let s_ = (lch.l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
        let rgb = [
            4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_,
            -1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_,
            -0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_,
        ];
        if rgb.iter().all(|v| (-0.0005..=1.0005).contains(v)) || c <= 0.0 {
            let byte = |v: f32| (to_gamma(v.clamp(0.0, 1.0)) * 255.0).round() as u8;
            return format!(
                "#{:02x}{:02x}{:02x}",
                byte(rgb[0]),
                byte(rgb[1]),
                byte(rgb[2])
            );
        }
        c = (c - 0.005).max(0.0);
    }
}

/// A cover's colours, as `#rrggbb`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Palette {
    /// The colour that is most the cover's; none for a black-and-white or grey cover.
    pub seed: Option<String>,
    /// Another colour of the cover, as far from the seed in hue as it has (30 degrees at least).
    pub second: Option<String>,
}

/// Hues counted in bins this wide (degrees); neighbours within 15 degrees share a score.
const BIN: f32 = 10.0;
const BINS: usize = 36;
/// Less colourful than this is grey (OKLCH chroma; Material's 5 in CAM16, roughly).
const GREY: f32 = 0.035;
/// A hue that covers less than this share of the cover (with its neighbours) never leads.
const SMALL: f32 = 0.02;

/// How much a pixel at (x, y) of a w×h cover counts: the middle fully, the edges less, and the
/// corners (where labels and badges sit) least of all.
fn weight(x: u32, y: u32, w: u32, h: u32) -> f32 {
    let fx = (x as f32 + 0.5) / w as f32;
    let fy = (y as f32 + 0.5) / h as f32;
    let dx = (fx - 0.5).abs() * 2.0;
    let dy = (fy - 0.5).abs() * 2.0;
    let corner = dx > 0.72 && dy > 0.72;
    let edge = 1.0 - 0.45 * dx.max(dy).powi(2);
    if corner { edge * 0.2 } else { edge }
}

#[derive(Clone, Copy, Default)]
struct Bin {
    w: f32,
    l: f32,
    a: f32,
    b: f32,
    c: f32,
}

pub fn palette(img: &RgbImage) -> Palette {
    let small = image::imageops::thumbnail(img, 64, 64);
    let (w, h) = small.dimensions();
    let mut bins = [Bin::default(); BINS];
    let mut total = 0.0f32;
    for (x, y, p) in small.enumerate_pixels() {
        let wt = weight(x, y, w, h);
        total += wt;
        let lab = oklab(p.0);
        let lch = lch_of(lab);
        // Greys, near-black and near-white say nothing about the cover's colour.
        if lch.c < GREY || lch.l < 0.18 || lch.l > 0.96 {
            continue;
        }
        let k = ((lch.h / BIN) as usize).min(BINS - 1);
        let b = &mut bins[k];
        b.w += wt;
        b.l += lab[0] * wt;
        b.a += lab[1] * wt;
        b.b += lab[2] * wt;
        b.c += lch.c * wt;
    }
    if total <= 0.0 {
        return Palette::default();
    }
    // Each hue with its neighbours: how much of the cover, and its colour.
    let mut cands: Vec<(f32, Lch, f32)> = Vec::new();
    for k in 0..BINS {
        let b = bins[k];
        if b.w <= 0.0 {
            continue;
        }
        let near: f32 = (-1i32..=1)
            .map(|d| bins[(k as i32 + d).rem_euclid(BINS as i32) as usize].w)
            .sum();
        let share = near / total;
        if share < SMALL {
            continue;
        }
        let lab = [b.l / b.w, b.a / b.w, b.b / b.w];
        let mean = lch_of(lab);
        // Its own pixels' chroma (the mean of a, b understates it when hues spread).
        let chroma = b.c / b.w;
        let colour = Lch {
            l: mean.l,
            c: chroma,
            h: mean.h,
        };
        // Material's score, with OKLCH chroma scaled to CAM16's range (sRGB red: 0.26 in OKLCH,
        // about 110 in CAM16).
        let c16 = chroma * 420.0;
        let score = share * 100.0 * 0.7 + (c16 - 48.0) * if c16 < 48.0 { 0.1 } else { 0.3 };
        cands.push((score, colour, share));
    }
    cands.sort_by(|a, b| b.0.total_cmp(&a.0));
    let Some(&(_, first, _)) = cands.first() else {
        return Palette::default();
    };
    let hue_gap = |a: f32, b: f32| {
        let d = (a - b).rem_euclid(360.0);
        d.min(360.0 - d)
    };
    // As far from the first as the cover allows: 90 degrees, then nearer, down to 30 (Material's
    // way), and only a colour that covers a real part of the cover.
    let second = [90.0, 75.0, 60.0, 45.0, 30.0].iter().find_map(|&min| {
        cands
            .iter()
            .skip(1)
            .find(|(_, c, share)| hue_gap(c.h, first.h) >= min && *share >= 0.05)
            .map(|(_, c, _)| hex_of(*c))
    });
    Palette {
        seed: Some(hex_of(first)),
        second,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    fn hue(hex: &str) -> f32 {
        let n = u32::from_str_radix(&hex[1..], 16).unwrap();
        lch_of(oklab([(n >> 16) as u8, (n >> 8) as u8, n as u8])).h
    }
    fn gap(a: f32, b: f32) -> f32 {
        let d = (a - b).rem_euclid(360.0);
        d.min(360.0 - d)
    }
    fn hue_of(rgb: [u8; 3]) -> f32 {
        lch_of(oklab(rgb)).h
    }

    #[test]
    fn a_badge_in_the_corner_does_not_win_over_the_cover() {
        // A calm blue cover with a loud red label badge in its top-right corner.
        let mut img = RgbImage::from_pixel(320, 320, Rgb([40, 70, 140]));
        for x in 270..315 {
            for y in 5..50 {
                img.put_pixel(x, y, Rgb([230, 20, 30]));
            }
        }
        let p = palette(&img);
        let seed = p.seed.expect("a colour");
        assert!(
            gap(hue(&seed), hue_of([40, 70, 140])) < 15.0,
            "seed {seed} should be the blue"
        );
        assert_eq!(
            p.second, None,
            "a corner badge is not the cover's second colour"
        );
    }

    #[test]
    fn two_colours_stay_two_colours_never_mud() {
        // Half orange, half teal: the old average was a brown neither of them.
        let mut img = RgbImage::from_pixel(320, 320, Rgb([230, 120, 30]));
        for x in 160..320 {
            for y in 0..320 {
                img.put_pixel(x, y, Rgb([20, 150, 150]));
            }
        }
        let p = palette(&img);
        let (a, b) = (hue(&p.seed.unwrap()), hue(&p.second.expect("both colours")));
        let (orange, teal) = (hue_of([230, 120, 30]), hue_of([20, 150, 150]));
        assert!(
            (gap(a, orange) < 15.0 && gap(b, teal) < 15.0)
                || (gap(a, teal) < 15.0 && gap(b, orange) < 15.0)
        );
    }

    #[test]
    fn a_grey_cover_has_no_colour() {
        let mut img = RgbImage::from_pixel(200, 200, Rgb([30, 30, 30]));
        for x in 50..150 {
            for y in 50..150 {
                img.put_pixel(x, y, Rgb([200, 200, 196]));
            }
        }
        assert_eq!(palette(&img), Palette::default());
    }

    #[test]
    fn the_colour_covering_most_leads_and_a_thin_detail_never_does() {
        // A dark green photo with a vivid yellow title across the middle third.
        let mut img = RgbImage::from_pixel(300, 300, Rgb([30, 70, 45]));
        for x in 0..300 {
            for y in 100..200 {
                img.put_pixel(x, y, Rgb([250, 210, 20]));
            }
        }
        // Two thirds green: the green leads (Material's way: how much of the cover first), and the
        // vivid yellow third is the second colour.
        let p = palette(&img);
        assert!(gap(hue(&p.seed.unwrap()), hue_of([30, 70, 45])) < 15.0);
        assert!(gap(hue(&p.second.expect("the yellow")), hue_of([250, 210, 20])) < 15.0);
        // A thin line of it (2%): the green does.
        let mut img = RgbImage::from_pixel(300, 300, Rgb([30, 90, 50]));
        for x in 0..300 {
            for y in 148..152 {
                img.put_pixel(x, y, Rgb([250, 210, 20]));
            }
        }
        let got = palette(&img).seed.unwrap();
        assert!(
            gap(hue(&got), hue_of([30, 90, 50])) < 15.0,
            "got {got} hue {} want {}",
            hue(&got),
            hue_of([30, 90, 50])
        );
    }

    #[test]
    fn colours_round_trip_through_oklch() {
        for rgb in [
            [250u8, 45, 72],
            [30, 215, 96],
            [20, 20, 20],
            [240, 240, 240],
        ] {
            let back = hex_of(lch_of(oklab(rgb)));
            assert_eq!(back, format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]));
        }
    }
}

#[cfg(test)]
mod sheet {
    use super::*;

    /// The old pick (averaging the most saturated pixels), for the comparison sheet only.
    fn old(img: &RgbImage) -> Option<String> {
        let small = image::imageops::thumbnail(img, 32, 32);
        let mut scored: Vec<(f32, [u8; 3])> = small
            .pixels()
            .map(|p| {
                let [r, g, b] = p.0;
                let (rf, gf, bf) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
                let max = rf.max(gf).max(bf);
                let min = rf.min(gf).min(bf);
                let sat = if max == 0.0 { 0.0 } else { (max - min) / max };
                let light = 1.0 - (max - 0.65).abs() * 1.6;
                (sat * light.max(0.0), [r, g, b])
            })
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        let top = &scored[..(scored.len() / 7).max(1)];
        if top[0].0 < 0.12 {
            return None;
        }
        let n = top.len() as u32;
        let s = top.iter().fold([0u32; 3], |a, (_, c)| {
            [a[0] + c[0] as u32, a[1] + c[1] as u32, a[2] + c[2] as u32]
        });
        Some(format!("#{:02x}{:02x}{:02x}", s[0] / n, s[1] / n, s[2] / n))
    }

    /// Old and new colours of real covers, side by side: run by hand with the thumbnails folder
    /// in VENUTA_THUMBS and the output file in VENUTA_SHEET (`-- --ignored`). Reads only.
    #[test]
    #[ignore]
    fn covers_sheet() {
        let dir = std::env::var("VENUTA_THUMBS").expect("thumbs");
        let out = std::env::var("VENUTA_SHEET").expect("out");
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.to_string_lossy().ends_with("-320.jpg"))
            .collect();
        files.sort();
        let step = (files.len() / 60).max(1);
        let rows: Vec<String> = files
            .iter()
            .step_by(step)
            .take(60)
            .map(|f| {
                let img = image::open(f).unwrap().to_rgb8();
                let p = palette(&img);
                format!(
                    "{{\"src\":{:?},\"old\":{:?},\"seed\":{:?},\"second\":{:?}}}",
                    f.to_string_lossy().replace('\\', "/"),
                    old(&img),
                    p.seed,
                    p.second
                )
            })
            .collect();
        std::fs::write(out, format!("[{}]", rows.join(","))).unwrap();
    }
}
