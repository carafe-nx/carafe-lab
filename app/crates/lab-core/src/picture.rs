//! Screenshots: reading PNG files and comparing two pictures of the screen.

use std::io::Read;

use flate2::read::ZlibDecoder;
use thiserror::Error;

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
const THUMB_WIDTH: usize = 64;
const THUMB_HEIGHT: usize = 36;
const CHANGE_THRESHOLD: f64 = 2.0;
const BLANK_SPREAD: f64 = 3.0;

/// An error reading a PNG file.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PictureError {
    /// The bytes are not a PNG file or it is damaged.
    #[error("not a PNG file: {0}")]
    Format(String),
    /// The PNG uses a layout the lab does not read: interlacing, palette or 16-bit samples.
    #[error("unsupported PNG layout: {0}")]
    Unsupported(String),
}

/// A picture reduced to brightness, 64×36 cells.
#[derive(Debug, Clone, PartialEq)]
pub struct Thumbnail {
    cells: Vec<f64>,
}

/// How two pictures differ.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Difference {
    /// Mean absolute brightness difference of the cells, 0…255.
    pub mean: f64,
}

impl Difference {
    /// Returns whether the screen visibly changed.
    #[must_use]
    pub fn changed(self) -> bool {
        self.mean > CHANGE_THRESHOLD
    }
}

impl Thumbnail {
    /// Reads a PNG file into a thumbnail.
    ///
    /// # Errors
    ///
    /// [`PictureError::Format`] if `png` is not a whole PNG file; [`PictureError::Unsupported`] if it is
    /// interlaced, uses a palette or 16-bit samples.
    pub fn from_png(png: &[u8]) -> Result<Self, PictureError> {
        let image = decode(png)?;
        Ok(Self::from_pixels(&image))
    }

    fn from_pixels(image: &Image) -> Self {
        let mut cells = vec![0.0; THUMB_WIDTH * THUMB_HEIGHT];
        let mut counts = vec![0u32; THUMB_WIDTH * THUMB_HEIGHT];
        for y in 0..image.height {
            let cy = y * THUMB_HEIGHT / image.height;
            for x in 0..image.width {
                let cx = x * THUMB_WIDTH / image.width;
                let at = (y * image.width + x) * image.channels;
                let luma = match image.channels {
                    1 | 2 => f64::from(image.pixels[at]),
                    _ => {
                        0.299 * f64::from(image.pixels[at])
                            + 0.587 * f64::from(image.pixels[at + 1])
                            + 0.114 * f64::from(image.pixels[at + 2])
                    }
                };
                cells[cy * THUMB_WIDTH + cx] += luma;
                counts[cy * THUMB_WIDTH + cx] += 1;
            }
        }
        for (cell, count) in cells.iter_mut().zip(counts) {
            if count > 0 {
                *cell /= f64::from(count);
            }
        }
        Self { cells }
    }

    /// Compares two thumbnails.
    #[must_use]
    pub fn difference(&self, other: &Self) -> Difference {
        let total: f64 = self
            .cells
            .iter()
            .zip(&other.cells)
            .map(|(a, b)| (a - b).abs())
            .sum();
        Difference {
            mean: total / self.cells.len() as f64,
        }
    }

    /// Returns whether the picture is one flat colour, such as a black screen.
    #[must_use]
    pub fn is_blank(&self) -> bool {
        let mean = self.cells.iter().sum::<f64>() / self.cells.len() as f64;
        let variance = self
            .cells
            .iter()
            .map(|cell| (cell - mean).powi(2))
            .sum::<f64>()
            / self.cells.len() as f64;
        variance.sqrt() < BLANK_SPREAD
    }
}

/// Colours the graphics probes draw: red, green, blue and white quadrants, left to right, top to bottom.
pub const PROBE_COLOURS: [[u8; 3]; 4] =
    [[217, 26, 26], [26, 191, 51], [38, 64, 230], [242, 242, 242]];
const PROBE_TOLERANCE: i32 = 40;

/// What a picture of a graphics probe shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternCheck {
    /// The typical colour of each quadrant, left to right, top to bottom.
    pub quadrants: [[u8; 3]; 4],
}

impl PatternCheck {
    /// Reads the quadrants of a PNG picture: the median colour of the middle half of each quadrant, so text over
    /// a corner, such as the DXVK HUD, does not count.
    ///
    /// # Errors
    ///
    /// [`PictureError`] if the PNG cannot be read.
    pub fn from_png(png: &[u8]) -> Result<Self, PictureError> {
        let image = decode(png)?;
        let mut quadrants = [[0u8; 3]; 4];
        for (index, quadrant) in quadrants.iter_mut().enumerate() {
            let (qx, qy) = (index % 2, index / 2);
            let (w, h) = (image.width / 2, image.height / 2);
            let (x0, y0) = (qx * w + w / 4, qy * h + h / 4);
            let mut channels: [Vec<u8>; 3] = [Vec::new(), Vec::new(), Vec::new()];
            for y in (y0..y0 + h / 2).step_by(4) {
                for x in (x0..x0 + w / 2).step_by(4) {
                    let at = (y * image.width + x) * image.channels;
                    for (channel, values) in channels.iter_mut().enumerate() {
                        let offset = if image.channels >= 3 { channel } else { 0 };
                        values.push(image.pixels[at + offset]);
                    }
                }
            }
            for (channel, values) in channels.iter_mut().enumerate() {
                values.sort_unstable();
                quadrant[channel] = values.get(values.len() / 2).copied().unwrap_or(0);
            }
        }
        Ok(Self { quadrants })
    }

    /// Returns whether every quadrant is close to the colour the probes draw there.
    #[must_use]
    pub fn matches_probe(&self) -> bool {
        self.quadrants
            .iter()
            .zip(PROBE_COLOURS)
            .all(|(seen, wanted)| {
                seen.iter()
                    .zip(wanted)
                    .all(|(&a, b)| (i32::from(a) - i32::from(b)).abs() <= PROBE_TOLERANCE)
            })
    }
}

struct Image {
    width: usize,
    height: usize,
    channels: usize,
    pixels: Vec<u8>,
}

fn decode(png: &[u8]) -> Result<Image, PictureError> {
    let rest = png
        .strip_prefix(&SIGNATURE)
        .ok_or_else(|| PictureError::Format("no PNG signature".to_owned()))?;
    let mut header = None;
    let mut data = Vec::new();
    let mut at = 0;
    while at + 8 <= rest.len() {
        let length =
            u32::from_be_bytes([rest[at], rest[at + 1], rest[at + 2], rest[at + 3]]) as usize;
        let kind = &rest[at + 4..at + 8];
        let body_end = at + 8 + length;
        if body_end + 4 > rest.len() {
            return Err(PictureError::Format("chunk runs past the end".to_owned()));
        }
        let body = &rest[at + 8..body_end];
        match kind {
            b"IHDR" => header = Some(read_header(body)?),
            b"IDAT" => data.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        at = body_end + 4;
    }
    let (width, height, channels) =
        header.ok_or_else(|| PictureError::Format("no IHDR chunk".to_owned()))?;
    let mut raw = Vec::new();
    ZlibDecoder::new(data.as_slice())
        .read_to_end(&mut raw)
        .map_err(|error| PictureError::Format(format!("image data: {error}")))?;
    let stride = width * channels;
    if raw.len() < height * (stride + 1) {
        return Err(PictureError::Format("image data is short".to_owned()));
    }
    let mut pixels = vec![0u8; height * stride];
    for y in 0..height {
        let line = &raw[y * (stride + 1)..(y + 1) * (stride + 1)];
        let (done, current) = pixels.split_at_mut(y * stride);
        let previous = if y == 0 {
            None
        } else {
            Some(&done[(y - 1) * stride..])
        };
        unfilter(
            line[0],
            &line[1..],
            previous,
            &mut current[..stride],
            channels,
        )?;
    }
    Ok(Image {
        width,
        height,
        channels,
        pixels,
    })
}

fn read_header(body: &[u8]) -> Result<(usize, usize, usize), PictureError> {
    if body.len() < 13 {
        return Err(PictureError::Format("short IHDR".to_owned()));
    }
    let width = u32::from_be_bytes([body[0], body[1], body[2], body[3]]) as usize;
    let height = u32::from_be_bytes([body[4], body[5], body[6], body[7]]) as usize;
    let (depth, colour, interlace) = (body[8], body[9], body[12]);
    if width == 0 || height == 0 {
        return Err(PictureError::Format("empty picture".to_owned()));
    }
    if depth != 8 {
        return Err(PictureError::Unsupported(format!("{depth}-bit samples")));
    }
    if interlace != 0 {
        return Err(PictureError::Unsupported("interlacing".to_owned()));
    }
    let channels = match colour {
        0 => 1,
        2 => 3,
        4 => 2,
        6 => 4,
        3 => return Err(PictureError::Unsupported("palette".to_owned())),
        other => return Err(PictureError::Format(format!("colour type {other}"))),
    };
    Ok((width, height, channels))
}

fn unfilter(
    filter: u8,
    line: &[u8],
    previous: Option<&[u8]>,
    out: &mut [u8],
    bpp: usize,
) -> Result<(), PictureError> {
    for i in 0..out.len() {
        let left = if i >= bpp { out[i - bpp] } else { 0 };
        let up = previous.map_or(0, |row| row[i]);
        let up_left = if i >= bpp {
            previous.map_or(0, |row| row[i - bpp])
        } else {
            0
        };
        let predicted = match filter {
            0 => 0,
            1 => left,
            2 => up,
            3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
            4 => paeth(left, up, up_left),
            other => return Err(PictureError::Format(format!("filter {other}"))),
        };
        out[i] = line[i].wrapping_add(predicted);
    }
    Ok(())
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let pa = (p - i16::from(a)).abs();
    let pb = (p - i16::from(b)).abs();
    let pc = (p - i16::from(c)).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::Compression;
    use flate2::write::ZlibEncoder;

    use super::*;

    fn png(width: u32, height: u32, filter: u8, pixel: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        let mut raw = Vec::new();
        for y in 0..height {
            raw.push(filter);
            let mut line = Vec::new();
            for x in 0..width {
                line.extend_from_slice(&pixel(x, y));
            }
            if filter == 1 {
                for i in (3..line.len()).rev() {
                    line[i] = line[i].wrapping_sub(line[i - 3]);
                }
            }
            raw.extend_from_slice(&line);
        }
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&raw).unwrap();
        let data = encoder.finish().unwrap();
        let mut header = Vec::new();
        header.extend_from_slice(&width.to_be_bytes());
        header.extend_from_slice(&height.to_be_bytes());
        header.extend_from_slice(&[8, 2, 0, 0, 0]);
        let mut file = SIGNATURE.to_vec();
        for (kind, body) in [(b"IHDR", header), (b"IDAT", data), (b"IEND", Vec::new())] {
            file.extend_from_slice(&(body.len() as u32).to_be_bytes());
            file.extend_from_slice(kind);
            file.extend_from_slice(&body);
            file.extend_from_slice(&[0, 0, 0, 0]);
        }
        file
    }

    #[test]
    fn a_black_screen_is_blank() {
        let black = Thumbnail::from_png(&png(128, 72, 0, |_, _| [0, 0, 0])).unwrap();
        assert!(black.is_blank());
    }

    #[test]
    fn a_picture_with_content_is_not_blank() {
        let split = png(
            128,
            72,
            0,
            |x, _| if x < 64 { [0, 0, 0] } else { [255, 255, 255] },
        );
        assert!(!Thumbnail::from_png(&split).unwrap().is_blank());
    }

    #[test]
    fn filters_decode_to_the_same_picture() {
        let gradient = |x: u32, y: u32| [x as u8, y as u8, (x + y) as u8];
        let plain = Thumbnail::from_png(&png(80, 40, 0, gradient)).unwrap();
        let sub = Thumbnail::from_png(&png(80, 40, 1, gradient)).unwrap();
        assert_eq!(plain, sub);
        assert!(!plain.difference(&sub).changed());
    }

    #[test]
    fn a_new_menu_is_a_change() {
        let before = Thumbnail::from_png(&png(128, 72, 0, |_, y| {
            if y < 36 { [40, 40, 40] } else { [0, 0, 0] }
        }))
        .unwrap();
        let after = Thumbnail::from_png(&png(128, 72, 0, |_, y| {
            if y < 36 { [0, 0, 0] } else { [200, 200, 200] }
        }))
        .unwrap();
        assert!(before.difference(&after).changed());
    }

    #[test]
    fn a_blinking_cursor_is_not_a_change() {
        let base = |x: u32, y: u32| {
            if x < 64 { [90, 90, 90] } else { [10, 10, 10] }.map(|v| v ^ u8::from(y == 0 && x == 0))
        };
        let before = Thumbnail::from_png(&png(128, 72, 0, base)).unwrap();
        let after = Thumbnail::from_png(&png(128, 72, 0, |x, y| {
            if x == 1 && y == 1 {
                [255, 255, 255]
            } else {
                base(x, y)
            }
        }))
        .unwrap();
        assert!(!before.difference(&after).changed());
    }

    #[test]
    fn rejects_what_is_not_png() {
        assert!(matches!(
            Thumbnail::from_png(b"GIF89a"),
            Err(PictureError::Format(_))
        ));
        let mut cut = png(8, 8, 0, |_, _| [1, 2, 3]);
        cut.truncate(30);
        assert!(matches!(
            Thumbnail::from_png(&cut),
            Err(PictureError::Format(_))
        ));
    }

    fn probe_picture(hud: bool) -> Vec<u8> {
        png(320, 180, 0, |x, y| {
            if hud && x < 120 && y < 60 && (x + y) % 3 == 0 {
                return [255, 255, 0];
            }
            let index = (if x < 160 { 0 } else { 1 }) + (if y < 90 { 0 } else { 2 });
            PROBE_COLOURS[index]
        })
    }

    #[test]
    fn the_probe_pattern_is_recognised() {
        let check = PatternCheck::from_png(&probe_picture(false)).unwrap();
        assert_eq!(check.quadrants, PROBE_COLOURS);
        assert!(check.matches_probe());
    }

    #[test]
    fn a_hud_over_the_corner_does_not_break_the_pattern() {
        assert!(
            PatternCheck::from_png(&probe_picture(true))
                .unwrap()
                .matches_probe()
        );
    }

    #[test]
    fn a_black_or_swapped_picture_is_not_the_pattern() {
        let black = png(320, 180, 0, |_, _| [0, 0, 0]);
        assert!(!PatternCheck::from_png(&black).unwrap().matches_probe());
        let swapped = png(320, 180, 0, |x, y| {
            let index = (if x < 160 { 1 } else { 0 }) + (if y < 90 { 0 } else { 2 });
            PROBE_COLOURS[index]
        });
        assert!(!PatternCheck::from_png(&swapped).unwrap().matches_probe());
    }

    #[test]
    fn rejects_palette_pictures() {
        let mut file = png(8, 8, 0, |_, _| [1, 2, 3]);
        file[8 + 8 + 9] = 3;
        assert!(matches!(
            Thumbnail::from_png(&file),
            Err(PictureError::Unsupported(_))
        ));
    }
}
