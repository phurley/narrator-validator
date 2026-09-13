//! Format 3.9 map SVG safety, viewport, and embedded-artwork checks.
use base64::{engine::general_purpose::STANDARD, Engine};
use image::{ImageFormat, ImageReader};
use std::io::Cursor;

pub const MAX_RASTER_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_RASTER_PIXELS: u64 = 8_388_608;
pub const MAX_RASTER_DIMENSION: u32 = 4096;
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapViewBox {
    pub min_x: f64,
    pub min_y: f64,
    pub width: f64,
    pub height: f64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapSvgProblem {
    pub code: &'static str,
    pub message: String,
}
fn problem(code: &'static str, message: impl Into<String>) -> MapSvgProblem {
    MapSvgProblem {
        code,
        message: message.into(),
    }
}

pub fn map_view_box(source: &str) -> Option<MapViewBox> {
    let document = roxmltree::Document::parse(source).ok()?;
    let root = document.root_element();
    (root.tag_name().name() == "svg")
        .then(|| root.attribute("viewBox"))
        .flatten()
        .and_then(|v| parse_view_box(v).ok())
}
fn parse_view_box(value: &str) -> Result<MapViewBox, ()> {
    let comma_shape = value
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect::<String>();
    if comma_shape.starts_with(',') || comma_shape.ends_with(',') || comma_shape.contains(",,") {
        return Err(());
    }
    let v = value
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|x| !x.is_empty())
        .map(str::parse::<f64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ())?;
    if v.len() != 4 || v.iter().any(|x| !x.is_finite()) || v[2] <= 0. || v[3] <= 0. {
        return Err(());
    }
    Ok(MapViewBox {
        min_x: v[0],
        min_y: v[1],
        width: v[2],
        height: v[3],
    })
}

/// Diagnostics never echo data URLs, preventing artwork from entering logs.
pub fn check_map_svg(source: &str) -> Vec<MapSvgProblem> {
    let mut out = Vec::new();
    let doc = match roxmltree::Document::parse(source) {
        Ok(doc) => doc,
        Err(e) => {
            out.push(problem(
                "case.map_svg_invalid",
                format!("map SVG is not well-formed XML: {e}"),
            ));
            return out;
        }
    };
    let root = doc.root_element();
    if root.tag_name().name() != "svg" {
        out.push(problem(
            "case.map_svg_root",
            format!(
                "map SVG root element must be `<svg>`, found `<{}>`",
                root.tag_name().name()
            ),
        ));
    } else if root
        .attribute("viewBox")
        .and_then(|v| parse_view_box(v).ok())
        .is_none()
    {
        out.push(problem("case.map_svg_view_box", "map SVG root `<svg>` needs exactly four finite viewBox numbers with positive width and height"));
    }
    let mut bytes = 0usize;
    let mut pixels = 0u64;
    for node in doc.descendants().filter(roxmltree::Node::is_element) {
        let name = node.tag_name().name();
        if matches!(name, "script" | "foreignObject") {
            out.push(problem(
                "case.map_svg_forbidden_element",
                format!("map SVG must not contain `<{name}>`"),
            ));
        }
        for attr in node.attributes() {
            let attr_name = attr.name();
            if attr_name.len() > 2 && attr_name[..2].eq_ignore_ascii_case("on") {
                out.push(problem(
                    "case.map_svg_event_attribute",
                    format!("map SVG must not declare event handler `{attr_name}` on `<{name}>`"),
                ));
            }
            if attr_name == "href" {
                if name == "image" && attr.value().starts_with("data:") {
                    check_raster(attr.value(), &mut bytes, &mut pixels, &mut out);
                } else if !attr.value().starts_with('#') {
                    out.push(problem("case.map_svg_external_reference", format!("map SVG `href` on `<{name}>` must be a same-document fragment or approved embedded image")));
                }
            }
        }
    }
    out
}
fn check_raster(
    value: &str,
    total_bytes: &mut usize,
    total_pixels: &mut u64,
    out: &mut Vec<MapSvgProblem>,
) {
    let (mime, encoded) = match value.split_once(";base64,") {
        Some(x) => x,
        None => {
            out.push(problem(
                "case.map_svg_image_data",
                "embedded artwork must use canonical base64 PNG or JPEG data URLs",
            ));
            return;
        }
    };
    let format = match mime {
        "data:image/png" => ImageFormat::Png,
        "data:image/jpeg" => ImageFormat::Jpeg,
        _ => {
            out.push(problem(
                "case.map_svg_image_data",
                "embedded artwork must declare image/png or image/jpeg",
            ));
            return;
        }
    };
    if encoded.is_empty()
        || encoded.len() % 4 != 0
        || !encoded
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='))
    {
        out.push(problem(
            "case.map_svg_image_data",
            "embedded artwork has invalid base64",
        ));
        return;
    }
    let data = match STANDARD.decode(encoded) {
        Ok(x) => x,
        Err(_) => {
            out.push(problem(
                "case.map_svg_image_data",
                "embedded artwork has invalid base64",
            ));
            return;
        }
    };
    if !raster_header_matches(format, &data) {
        out.push(problem(
            "case.map_svg_image_invalid",
            "embedded artwork does not match its declared image type",
        ));
        return;
    }
    if exceeds_limit(
        *total_bytes as u64,
        data.len() as u64,
        MAX_RASTER_BYTES as u64,
    ) {
        out.push(problem(
            "case.map_svg_image_bytes",
            format!("embedded raster artwork exceeds the {MAX_RASTER_BYTES}-byte decoded limit"),
        ));
        return;
    }
    let (w, h) = match ImageReader::with_format(Cursor::new(&data), format).into_dimensions() {
        Ok(x) => x,
        Err(_) => {
            out.push(problem(
                "case.map_svg_image_invalid",
                "embedded artwork is not a complete valid image of its declared type",
            ));
            return;
        }
    };
    if w == 0 || h == 0 || w > MAX_RASTER_DIMENSION || h > MAX_RASTER_DIMENSION {
        out.push(problem(
            "case.map_svg_image_dimensions",
            format!("embedded artwork dimensions must be 1..={MAX_RASTER_DIMENSION}"),
        ));
        return;
    }
    let count = u64::from(w) * u64::from(h);
    if exceeds_limit(*total_pixels, count, MAX_RASTER_PIXELS) {
        out.push(problem(
            "case.map_svg_image_pixels",
            format!("embedded raster artwork exceeds the {MAX_RASTER_PIXELS}-pixel limit"),
        ));
        return;
    }
    if format == ImageFormat::Jpeg && jpeg_has_nonidentity_orientation(&data) {
        out.push(problem(
            "case.map_svg_image_orientation",
            "embedded JPEG has a non-identity EXIF orientation; export it upright before embedding",
        ));
        return;
    }
    // Header dimensions make the allocation bounds-safe; decode catches truncation.
    if ImageReader::with_format(Cursor::new(&data), format)
        .decode()
        .is_err()
    {
        out.push(problem(
            "case.map_svg_image_invalid",
            "embedded artwork is not a complete valid image of its declared type",
        ));
        return;
    }
    *total_bytes += data.len();
    *total_pixels += count;
}
fn exceeds_limit(total: u64, incoming: u64, limit: u64) -> bool {
    total.checked_add(incoming).map_or(true, |sum| sum > limit)
}
fn raster_header_matches(format: ImageFormat, data: &[u8]) -> bool {
    match format {
        ImageFormat::Png => data.starts_with(b"\x89PNG\r\n\x1a\n"),
        ImageFormat::Jpeg => data.starts_with(&[0xff, 0xd8]),
        _ => false,
    }
}
fn jpeg_has_nonidentity_orientation(b: &[u8]) -> bool {
    if b.get(..2) != Some(&[0xff, 0xd8]) {
        return false;
    }
    let mut i = 2;
    while i + 4 <= b.len() {
        if b[i] != 0xff {
            return false;
        }
        while i < b.len() && b[i] == 0xff {
            i += 1;
        }
        if i >= b.len() || matches!(b[i], 0xda | 0xd9) {
            break;
        }
        let n = u16::from_be_bytes([b[i + 1], b[i + 2]]) as usize;
        if n < 2 || i + 1 + n > b.len() {
            break;
        }
        if b[i] == 0xe1 && b.get(i + 3..i + 9) == Some(b"Exif\0\0") {
            return exif_orientation(&b[i + 9..i + 1 + n]).is_some_and(|v| v != 1);
        }
        i += 1 + n;
    }
    false
}
fn exif_orientation(t: &[u8]) -> Option<u16> {
    let le = match t.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16at = |p| {
        let x = t.get(p..p + 2)?;
        Some(if le {
            u16::from_le_bytes([x[0], x[1]])
        } else {
            u16::from_be_bytes([x[0], x[1]])
        })
    };
    let u32at = |p| {
        let x = t.get(p..p + 4)?;
        Some(if le {
            u32::from_le_bytes([x[0], x[1], x[2], x[3]])
        } else {
            u32::from_be_bytes([x[0], x[1], x[2], x[3]])
        } as usize)
    };
    let p = u32at(4)?;
    for n in 0..u16at(p)? as usize {
        let e = p.checked_add(2 + n * 12)?;
        if u16at(e)? == 0x0112 && u16at(e + 2)? == 3 && u32at(e + 4)? == 1 {
            return u16at(e + 8);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageBuffer, Rgb, Rgba};
    fn codes(source: &str) -> Vec<&'static str> {
        check_map_svg(source).into_iter().map(|p| p.code).collect()
    }
    fn encoded_png(width: u32, height: u32) -> Vec<u8> {
        let image =
            DynamicImage::ImageRgba8(ImageBuffer::from_pixel(width, height, Rgba([0, 0, 0, 255])));
        let mut bytes = Cursor::new(Vec::new());
        image
            .write_to(&mut bytes, ImageFormat::Png)
            .expect("test image encodes");
        bytes.into_inner()
    }
    fn jpeg_with_orientation(orientation: u8) -> Vec<u8> {
        let image = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(1, 1, Rgb([0, 0, 0])));
        let mut encoded = Cursor::new(Vec::new());
        image
            .write_to(&mut encoded, ImageFormat::Jpeg)
            .expect("test image encodes");
        // A minimal little-endian EXIF IFD0 containing tag 0x0112 (orientation).
        let mut app1 = vec![
            0xff,
            0xe1,
            0x00,
            0x22,
            b'E',
            b'x',
            b'i',
            b'f',
            0,
            0,
            b'I',
            b'I',
            42,
            0,
            8,
            0,
            0,
            0,
            1,
            0,
            0x12,
            0x01,
            3,
            0,
            1,
            0,
            0,
            0,
            orientation,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ];
        app1.extend(encoded.into_inner().split_off(2));
        let mut jpeg = vec![0xff, 0xd8];
        jpeg.append(&mut app1);
        jpeg
    }
    const SAFE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 60"><rect id="parlor"/><use href="#parlor"/></svg>"##;
    #[test]
    fn retains_existing_svg_safety_guards() {
        assert!(check_map_svg(SAFE).is_empty());
        assert_eq!(codes("<svg viewBox=\"0 0 1 1\">"), ["case.map_svg_invalid"]);
        assert_eq!(
            codes(
                r#"<!DOCTYPE svg [<!ENTITY a "x">]><svg viewBox="0 0 1 1"><title>&a;</title></svg>"#
            ),
            ["case.map_svg_invalid"]
        );
        assert_eq!(
            codes(&SAFE.replace("<rect", "<script>x</script><rect")),
            ["case.map_svg_forbidden_element"]
        );
        assert_eq!(
            codes(&SAFE.replace("<rect", "<rect onclick=\"x()\"")),
            ["case.map_svg_event_attribute"]
        );
        assert_eq!(
            codes(&SAFE.replace("href=\"#parlor\"", "href=\"https://example.test/a.svg\"")),
            ["case.map_svg_external_reference"]
        );
    }
    #[test]
    fn accepts_negative_origin_and_svg_separators() {
        assert_eq!(
            map_view_box(r#"<svg viewBox="-100, 20 800,600"/>"#),
            Some(MapViewBox {
                min_x: -100.,
                min_y: 20.,
                width: 800.,
                height: 600.
            })
        );
    }
    #[test]
    fn rejects_bad_viewbox_and_unsafe_references() {
        assert_eq!(
            codes(r#"<svg viewBox="0 0 1"><image href="https://example.test/a.png"/></svg>"#),
            vec!["case.map_svg_view_box", "case.map_svg_external_reference"]
        );
        assert_eq!(
            codes(r#"<svg viewBox=",0,,0,1,1,"/>"#),
            vec!["case.map_svg_view_box"]
        );
    }
    #[test]
    fn rejects_noncanonical_embedded_images_without_echoing_them() {
        let problems = check_map_svg(
            r#"<svg viewBox="0 0 1 1"><image href="data:image/png;base64,aGVsbG8"/></svg>"#,
        );
        assert_eq!(problems[0].code, "case.map_svg_image_data");
        assert!(!problems[0].message.contains("aGVsbG8"));
    }
    #[test]
    fn decodes_valid_png_and_jpeg_embedded_artwork() {
        for (mime, format) in [
            ("image/png", ImageFormat::Png),
            ("image/jpeg", ImageFormat::Jpeg),
        ] {
            let image = match format {
                ImageFormat::Jpeg => {
                    DynamicImage::ImageRgb8(ImageBuffer::from_pixel(1, 1, Rgb([0, 0, 0])))
                }
                _ => DynamicImage::ImageRgba8(ImageBuffer::from_pixel(1, 1, Rgba([0, 0, 0, 255]))),
            };
            let mut bytes = Cursor::new(Vec::new());
            image
                .write_to(&mut bytes, format)
                .expect("test image encodes");
            let source = format!(
                r#"<svg viewBox="0 0 1 1"><image href="data:{mime};base64,{}"/></svg>"#,
                STANDARD.encode(bytes.into_inner())
            );
            assert!(check_map_svg(&source).is_empty(), "{mime}");
        }
    }
    #[test]
    fn rejects_mismatched_truncated_and_oversized_embedded_artwork() {
        let png = encoded_png(1, 1);
        let encoded = STANDARD.encode(&png);
        assert_eq!(
            codes(&format!(
                r#"<svg viewBox="0 0 1 1"><image href="data:image/jpeg;base64,{encoded}"/></svg>"#
            )),
            ["case.map_svg_image_invalid"]
        );
        assert_eq!(
            codes(&format!(
                r#"<svg viewBox="0 0 1 1"><image href="data:image/png;base64,{}"/></svg>"#,
                STANDARD.encode(&png[..png.len() / 2])
            )),
            ["case.map_svg_image_invalid"]
        );
        let mut problems = Vec::new();
        check_raster(
            &format!("data:image/png;base64,{encoded}"),
            &mut (MAX_RASTER_BYTES - png.len() + 1),
            &mut 0,
            &mut problems,
        );
        assert_eq!(problems[0].code, "case.map_svg_image_bytes");
        let mut problems = Vec::new();
        let mut pixels = MAX_RASTER_PIXELS;
        check_raster(
            &format!("data:image/png;base64,{encoded}"),
            &mut 0,
            &mut pixels,
            &mut problems,
        );
        assert_eq!(problems[0].code, "case.map_svg_image_pixels");
        assert_eq!(
            codes(&format!(
                r#"<svg viewBox="0 0 1 1"><image href="data:image/png;base64,{}"/></svg>"#,
                STANDARD.encode(encoded_png(MAX_RASTER_DIMENSION + 1, 1))
            )),
            ["case.map_svg_image_dimensions"]
        );
    }
    #[test]
    fn aggregate_raster_bounds_reject_an_oversized_first_image() {
        assert!(!exceeds_limit(
            0,
            MAX_RASTER_BYTES as u64,
            MAX_RASTER_BYTES as u64
        ));
        assert!(exceeds_limit(
            0,
            MAX_RASTER_BYTES as u64 + 1,
            MAX_RASTER_BYTES as u64
        ));
        assert!(!exceeds_limit(0, MAX_RASTER_PIXELS, MAX_RASTER_PIXELS));
        assert!(exceeds_limit(0, MAX_RASTER_PIXELS + 1, MAX_RASTER_PIXELS));

        // The byte gate runs before image parsing, so this fixture proves a
        // first oversized image cannot reach a decoder allocation.
        let mut image = vec![0; MAX_RASTER_BYTES + 1];
        image[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        let mut problems = Vec::new();
        check_raster(
            &format!("data:image/png;base64,{}", STANDARD.encode(image)),
            &mut 0,
            &mut 0,
            &mut problems,
        );
        assert_eq!(problems[0].code, "case.map_svg_image_bytes");
    }
    #[test]
    fn rejects_nonidentity_jpeg_exif_orientation() {
        assert_eq!(
            codes(&format!(
                r#"<svg viewBox="0 0 1 1"><image href="data:image/jpeg;base64,{}"/></svg>"#,
                STANDARD.encode(jpeg_with_orientation(6))
            )),
            ["case.map_svg_image_orientation"]
        );
    }
}
