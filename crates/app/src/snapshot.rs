//! Development aid: save the window's own rendering as a PNG.

use std::fs::File;
use std::io::BufWriter;

use slint::{Rgba8Pixel, SharedPixelBuffer};

pub fn save_png(path: &str, buf: &SharedPixelBuffer<Rgba8Pixel>) -> Result<(), String> {
    let f = File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(BufWriter::new(f), buf.width(), buf.height());
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(|e| e.to_string())?;
    w.write_image_data(buf.as_bytes()).map_err(|e| e.to_string())
}
