//! PNG and vector PDF files from a blueprint SVG drawn in millimetres.
//!
//! The SVG root gives `width`/`height` in `mm` with a viewBox in the same
//! millimetres. usvg resolves `mm` at 96 CSS pixels per inch; the PDF page is
//! written at that resolution, so one SVG millimetre is 72/25.4 pt on the page
//! and a print at actual size keeps hole pitch and rulers 1:1.

use std::sync::{Arc, OnceLock};

use resvg::{tiny_skia, usvg};

/// CSS pixels per inch, the resolution usvg resolves `mm` lengths at.
const CSS_DPI: f32 = 96.;
/// Largest PNG accepted, in pixels (64 MP).
const MAX_PIXELS: f64 = 64e6;
/// Highest PNG resolution accepted, in pixels per millimetre (1016 dpi).
pub(crate) const MAX_PX_PER_MM: f64 = 40.;
/// PNG resolution when none is given: 4 px/mm, about 102 dpi.
pub(crate) const DEFAULT_PX_PER_MM: f64 = 4.;

/// System fonts, loaded once, so labels convert to glyphs.
fn fonts() -> Arc<usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut fonts = usvg::fontdb::Database::new();
            fonts.load_system_fonts();
            Arc::new(fonts)
        })
        .clone()
}

fn tree(svg: &str) -> Result<usvg::Tree, String> {
    let options = usvg::Options {
        fontdb: fonts(),
        ..usvg::Options::default()
    };
    usvg::Tree::from_str(svg, &options)
        .map_err(|e| format!("Could not read the blueprint SVG: {e}"))
}

/// Page width and height in millimetres, read exactly from the root `width`
/// and `height` (both in `mm`). usvg keeps sizes as `f32`, which is too coarse
/// for the page box and for pixel rounding, so neither is taken from the tree.
pub(crate) fn page_mm(svg: &str) -> Result<[f64; 2], String> {
    let start = svg
        .find("<svg")
        .ok_or("The blueprint SVG has no root element")?;
    let root = &svg[start
        ..start
            + svg[start..]
                .find('>')
                .ok_or("The blueprint SVG root is not closed")?];
    let length = |name: &str| {
        let at = root
            .find(&format!(" {name}=\""))
            .map(|at| at + name.len() + 3);
        at.and_then(|at| root[at..].find('"').map(|end| &root[at..at + end]))
            .and_then(|value| value.strip_suffix("mm"))
            .and_then(|value| value.parse::<f64>().ok())
            .filter(|mm| mm.is_finite() && *mm > 0.)
            .ok_or_else(|| format!("The blueprint SVG root needs a positive {name} in mm"))
    };
    Ok([length("width")?, length("height")?])
}

/// Vector PDF with one page the drawing's physical size: pt = mm·72/25.4.
/// Lines and fills stay vector paths and text is embedded as glyphs; nothing
/// is rasterised. `compress` deflates the content streams.
pub(crate) fn pdf(svg: &str, compress: bool) -> Result<Vec<u8>, String> {
    let mm = page_mm(svg)?;
    let options = svg2pdf::ConversionOptions {
        compress,
        ..svg2pdf::ConversionOptions::default()
    };
    let pdf = svg2pdf::to_pdf(&tree(svg)?, options, svg2pdf::PageOptions { dpi: CSS_DPI })
        .map_err(|e| format!("Could not write the blueprint PDF: {e}"))?;
    exact_media_box(pdf, mm.map(|mm| mm * 72. / 25.4))
}

/// Rewrites the single page's `/MediaBox` with the exact size in points.
/// pdf-writer stores reals as `f32` (about 2e-4 pt apart near 3000 pt); the
/// page box is rewritten to 1e-8 pt and the cross-reference offsets after it
/// shifted by the length change. The content keeps its `f32` transform, a
/// relative error near 1e-7.
fn exact_media_box(mut pdf: Vec<u8>, [width, height]: [f64; 2]) -> Result<Vec<u8>, String> {
    let broken = |what: &str| format!("Could not set the PDF page size: {what}");
    let find = |bytes: &[u8], needle: &[u8], from: usize| {
        bytes[from..]
            .windows(needle.len())
            .position(|w| w == needle)
            .map(|at| from + at)
    };
    let key = b"/MediaBox [";
    let start = find(&pdf, key, 0).ok_or_else(|| broken("no MediaBox"))? + key.len();
    if find(&pdf, key, start).is_some() {
        return Err(broken("more than one MediaBox"));
    }
    let end = find(&pdf, b"]", start).ok_or_else(|| broken("open MediaBox"))?;
    let exact = format!("0 0 {width:.8} {height:.8}");
    let delta = exact.len() as i64 - (end - start) as i64;
    pdf.splice(start..end, exact.into_bytes());
    let shift = |offset: usize| {
        if offset > start {
            (offset as i64 + delta) as usize
        } else {
            offset
        }
    };
    // Classic table: "xref\n0 N\n" then N 20-byte entries "oooooooooo ggggg n\r\n".
    let xref =
        find(&pdf, b"\nxref\n0 ", start).ok_or_else(|| broken("no cross-reference table"))? + 1;
    let header_end =
        find(&pdf, b"\n", xref + 5).ok_or_else(|| broken("cross-reference header"))? + 1;
    let count: usize = std::str::from_utf8(&pdf[xref + 7..header_end - 1])
        .ok()
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| broken("cross-reference size"))?;
    for i in 0..count {
        let entry = header_end + 20 * i;
        let line = pdf
            .get(entry..entry + 20)
            .ok_or_else(|| broken("short cross-reference table"))?;
        if line[17] != b'n' {
            continue;
        }
        let offset: usize = std::str::from_utf8(&line[..10])
            .ok()
            .and_then(|n| n.parse().ok())
            .ok_or_else(|| broken("cross-reference entry"))?;
        pdf[entry..entry + 10].copy_from_slice(format!("{:010}", shift(offset)).as_bytes());
    }
    let marker = b"startxref\n";
    let at = find(&pdf, marker, header_end).ok_or_else(|| broken("no startxref"))? + marker.len();
    let stop = find(&pdf, b"\n", at).ok_or_else(|| broken("open startxref"))?;
    let old: usize = std::str::from_utf8(&pdf[at..stop])
        .ok()
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| broken("startxref value"))?;
    pdf.splice(at..stop, shift(old).to_string().into_bytes());
    Ok(pdf)
}

/// PNG pixel width and height for a page of `size_mm` at `px_per_mm`: each
/// side is rounded to the nearest whole pixel, at least one.
pub(crate) fn png_size(size_mm: [f64; 2], px_per_mm: f64) -> [u32; 2] {
    size_mm.map(|mm| (mm * px_per_mm).round().max(1.) as u32)
}

/// PNG of the drawing on a white ground at `px_per_mm`; see [`png_size`].
pub(crate) fn png(svg: &str, px_per_mm: f64) -> Result<Vec<u8>, String> {
    if !(px_per_mm.is_finite() && px_per_mm > 0. && px_per_mm <= MAX_PX_PER_MM) {
        return Err(format!(
            "px_per_mm must be greater than 0 and at most {MAX_PX_PER_MM}, got {px_per_mm}"
        ));
    }
    let mm = page_mm(svg)?;
    let [width, height] = png_size(mm, px_per_mm);
    if f64::from(width) * f64::from(height) > MAX_PIXELS {
        return Err(format!(
            "A {:.0} × {:.0} mm drawing at {px_per_mm} px/mm is {width} × {height} px, over the {} MP limit; lower px_per_mm",
            mm[0],
            mm[1],
            MAX_PIXELS / 1e6
        ));
    }
    let tree = tree(svg)?;
    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or("Could not allocate the PNG")?;
    pixmap.fill(tiny_skia::Color::WHITE);
    let size = tree.size();
    let scale = tiny_skia::Transform::from_scale(
        width as f32 / size.width(),
        height as f32 / size.height(),
    );
    resvg::render(&tree, scale, &mut pixmap.as_mut());
    pixmap
        .encode_png()
        .map_err(|e| format!("Could not encode the blueprint PNG: {e}"))
}

/// Standard base64 with padding.
pub(crate) fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            out.push(if i <= chunk.len() {
                ALPHABET[(n >> shift & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optics::{System, bench_lab};

    /// The blueprint SVG of a template and its page size (mm) read from the root.
    fn blueprint(name: &str) -> (String, [f64; 2]) {
        blueprint_of(&bench_lab::template(name).unwrap())
    }

    fn blueprint_of(bench: &crate::optics::bench::Bench) -> (String, [f64; 2]) {
        let svg = bench_lab::export_blueprint(bench, &System::default(), "svg").unwrap()["content"]
            .as_str()
            .unwrap()
            .to_owned();
        let root = &svg[..svg.find('>').unwrap()];
        let attr = |name: &str| {
            let at = root.find(&format!(" {name}=\"")).unwrap() + name.len() + 3;
            root[at..at + root[at..].find('"').unwrap()].to_owned()
        };
        let mm = |name: &str| {
            attr(name)
                .strip_suffix("mm")
                .unwrap_or_else(|| panic!("{name} not in mm"))
                .parse::<f64>()
                .unwrap()
        };
        let size = [mm("width"), mm("height")];
        // The viewBox is in the same millimetres as the page.
        let view: Vec<f64> = attr("viewBox")
            .split(' ')
            .map(|v| v.parse().unwrap())
            .collect();
        assert_eq!(
            [view[2], view[3]],
            size,
            "viewBox size differs from the page size"
        );
        assert_eq!(page_mm(&svg).unwrap(), size);
        (svg, size)
    }

    #[test]
    fn the_svg_root_is_in_millimetres_with_a_millimetre_viewbox() {
        for name in bench_lab::TEMPLATES {
            let (_, [w, h]) = blueprint(name);
            assert!(w > 100. && h > 100., "{name}: {w} × {h} mm");
        }
    }

    /// A delay line whose page is `width` mm wide: the page adds 64 mm of margin.
    fn wide_delay_line(width: f64) -> (String, [f64; 2]) {
        let mut bench = bench_lab::template("delay_line").unwrap();
        bench.table_size[0] = width - 64.;
        let drawn = blueprint_of(&bench);
        assert_eq!(drawn.1[0], width);
        drawn
    }

    /// Every in-use cross-reference offset points at its `n 0 obj`, and
    /// `startxref` at the table.
    fn assert_cross_references(pdf: &[u8]) {
        let last = |needle: &[u8]| {
            pdf.windows(needle.len())
                .rposition(|w| w == needle)
                .unwrap()
        };
        let number = |bytes: &[u8]| {
            std::str::from_utf8(bytes)
                .unwrap()
                .trim()
                .parse::<usize>()
                .unwrap()
        };
        let xref = last(b"\nxref\n") + 1;
        let at = last(b"startxref\n") + 10;
        assert_eq!(
            number(&pdf[at..at + pdf[at..].iter().position(|&b| b == b'\n').unwrap()]),
            xref
        );
        let entries = xref + pdf[xref..].iter().position(|&b| b == b'\n').unwrap() + 1;
        let entries = entries + pdf[entries..].iter().position(|&b| b == b'\n').unwrap() + 1;
        let mut id = 0;
        while pdf[entries + 20 * id..].starts_with(b"0") {
            let entry = &pdf[entries + 20 * id..entries + 20 * id + 20];
            if entry[17] == b'n' {
                let offset = number(&entry[..10]);
                assert!(
                    pdf[offset..].starts_with(format!("{id} 0 obj").as_bytes()),
                    "object {id} at {offset}"
                );
            }
            id += 1;
        }
        assert!(id > 3, "{id} cross-reference entries");
    }

    #[test]
    fn the_pdf_page_is_the_drawing_size_and_holds_vector_paths_only() {
        // 975 mm: 2763.779527559… pt, where an f32 page box is off by ~2e-5 pt.
        let (svg, [w, h]) = wide_delay_line(975.);
        let pdf = pdf(&svg, true).unwrap();
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.starts_with("%PDF-"));
        let at = text.find("/MediaBox [").unwrap() + "/MediaBox [".len();
        let media: Vec<f64> = text[at..at + text[at..].find(']').unwrap()]
            .split_whitespace()
            .map(|v| v.parse().unwrap())
            .collect();
        let expected = [0., 0., w * 72. / 25.4, h * 72. / 25.4];
        for (got, want) in media.iter().zip(expected) {
            assert!(
                (got - want).abs() <= 1e-6,
                "MediaBox {media:?} vs {expected:?}"
            );
        }
        assert_cross_references(&pdf);
        assert_eq!(
            text.matches("/Type /Page\n").count()
                + text.matches("/Type /Page/").count()
                + text.matches("/Type /Page ").count(),
            1,
            "one page"
        );
        assert!(
            !text.contains("/Subtype /Image"),
            "no embedded raster image"
        );
        // The same conversion uncompressed: path construction and painting operators.
        let plain = super::pdf(&svg, false).unwrap();
        assert_cross_references(&plain);
        let plain = String::from_utf8_lossy(&plain).into_owned();
        assert!(!plain.contains("/Subtype /Image"));
        let ops = |op: &str| plain.lines().filter(|l| l.ends_with(op)).count();
        assert!(
            ops(" m") > 100 && ops(" l") > 100,
            "move/line operators: {} {}",
            ops(" m"),
            ops(" l")
        );
        assert!(ops("S") + ops("f") > 100, "stroke/fill operators");
    }

    #[test]
    fn the_png_is_the_drawing_size_times_px_per_mm_and_not_blank() {
        let (svg, mm) = blueprint("delay_line");
        let px_per_mm = 1.5;
        let png = png(&svg, px_per_mm).unwrap();
        let image = tiny_skia::Pixmap::decode_png(&png).unwrap();
        assert_eq!(
            [image.width(), image.height()],
            mm.map(|v| (v * px_per_mm).round() as u32)
        );
        let pixels = image.pixels();
        let white = pixels
            .iter()
            .filter(|p| (p.red(), p.green(), p.blue(), p.alpha()) == (255, 255, 255, 255))
            .count();
        assert!(
            pixels.iter().all(|p| p.alpha() == 255),
            "opaque white ground"
        );
        assert!(
            white < pixels.len() && white * 10 > pixels.len(),
            "{white} of {} pixels white",
            pixels.len()
        );
        // The table plate is drawn: a tinted, non-white area of many pixels.
        assert!(
            pixels.len() - white > pixels.len() / 10,
            "drawing covers too little: {}",
            pixels.len() - white
        );
    }

    #[test]
    fn a_half_pixel_page_side_rounds_up_through_the_whole_png_export() {
        // 975 mm × 1.5 px/mm = 1462.5 px → 1463; usvg's f32 width alone gives 1462.
        let (svg, [_, h]) = wide_delay_line(975.);
        let image = tiny_skia::Pixmap::decode_png(&png(&svg, 1.5).unwrap()).unwrap();
        assert_eq!(
            [image.width(), image.height()],
            [1463, (h * 1.5).round() as u32]
        );
    }

    #[test]
    fn png_size_rounds_each_side_and_png_refuses_bad_resolutions() {
        assert_eq!(png_size([100.4, 0.1], 1.), [100, 1]);
        assert_eq!(png_size([100.5, 10.], 2.), [201, 20]);
        let (svg, _) = blueprint("michelson");
        for bad in [0., -1., f64::NAN, MAX_PX_PER_MM + 1.] {
            assert!(
                png(&svg, bad).unwrap_err().contains("px_per_mm must be"),
                "{bad}"
            );
        }
        assert!(png(&svg, MAX_PX_PER_MM).unwrap_err().contains("MP limit"));
    }

    #[test]
    fn base64_matches_the_rfc_4648_vectors() {
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(plain.as_bytes()), encoded);
        }
    }
}
