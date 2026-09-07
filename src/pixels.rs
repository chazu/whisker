//! Drawing the view grid as a small inline image.
//!
//! The grid is a picture, so it is drawn as one: a dot per node, colour
//! carrying state, sized to a whole number of terminal cells. See
//! `docs/grid-design.md` for why this beats a row of text characters, and
//! `docs/probes/pixelgrid.rs` for the measurements behind it.
//!
//! Two constraints shape everything here.
//!
//! Readline needs the printing width of the prompt. An image has none, so the
//! placement declares an exact cell extent with `c=`/`r=` and pins the cursor
//! with `C=1`; the caller then emits that many real spaces, which Readline can
//! count. The declared width is therefore true by construction rather than
//! estimated.
//!
//! And the terminal scales the image into a box measured in *points*, not
//! device pixels. On a 2x display a dot of N image pixels is drawn N/2 points
//! wide, so sizes given in the configuration are doubled here to compensate.
//!
//! No dependencies: PNG needs a zlib stream, so this carries a small
//! fixed-Huffman deflate encoder. Stored blocks were tried first and produced
//! 11841 bytes for one grid against 991 compressed, which is far too much to
//! emit on every prompt.

use crate::config::{Config, State};

/// The terminal's pixels per cell, from `TIOCGWINSZ`.
///
/// Sizing the image to a whole number of cells needs this, and not every
/// terminal reports it: iTerm2 returns zeroes where Ghostty gives 16x34. A
/// terminal that will not say is not a terminal we can draw in, which is a
/// detectable condition rather than a guess, so the caller falls back to text.
///
/// Whisker's own output is captured into a shell variable, so stdout is a pipe
/// by the time this runs. The controlling terminal is asked directly instead.
pub fn cell_size() -> Option<(usize, usize)> {
    #[repr(C)]
    struct WinSize {
        rows: u16,
        columns: u16,
        x_pixels: u16,
        y_pixels: u16,
    }
    // SAFETY: the struct matches the kernel's winsize layout, and the ioctl
    // only writes into it. A failed open or ioctl is handled as "unknown".
    unsafe {
        unsafe extern "C" {
            fn open(path: *const u8, flags: i32) -> i32;
            fn close(fd: i32) -> i32;
            fn ioctl(fd: i32, request: u64, ...) -> i32;
        }
        // The request number is not portable: it encodes the struct size and
        // direction on the BSDs, and is a small constant on Linux.
        #[cfg(target_os = "linux")]
        const TIOCGWINSZ: u64 = 0x5413;
        #[cfg(not(target_os = "linux"))]
        const TIOCGWINSZ: u64 = 0x4008_7468;
        let fd = open(c"/dev/tty".as_ptr() as *const u8, 0);
        if fd < 0 {
            return None;
        }
        let mut size = WinSize { rows: 0, columns: 0, x_pixels: 0, y_pixels: 0 };
        let result = ioctl(fd, TIOCGWINSZ, &mut size as *mut WinSize);
        close(fd);
        if result != 0 || size.rows == 0 || size.columns == 0 {
            return None;
        }
        // Zeroes here mean the terminal declines to say, which is the case
        // that must fall back rather than divide by zero.
        if size.x_pixels == 0 || size.y_pixels == 0 {
            return None;
        }
        Some((
            size.x_pixels as usize / size.columns as usize,
            size.y_pixels as usize / size.rows as usize,
        ))
    }
}

/// A node's colour. Idle is the muted default, so a grid at rest is quiet and
/// an alert is the only thing that draws the eye.
fn rgba(state: State, current: bool) -> [u8; 4] {
    if current {
        return [240, 240, 240, 255];
    }
    match state {
        State::Unknown => [70, 70, 80, 255],
        State::Ok => [90, 140, 90, 255],
        State::Alert => [230, 70, 60, 255],
    }
}

/// Paint the grid and wrap it in a kitty placement, or `None` when it does not
/// fit the space allowed.
///
/// Refusing is deliberate and is the same rule `atomic` expresses for text: a
/// grid drawn partially still looks like a grid while hiding whatever fell off
/// the edge, and a display that hides an alert is worse than one that is
/// absent, because the user reads calm and believes it.
pub fn placement(
    config: &Config,
    current: &str,
    cell: (usize, usize),
    cells: usize,
    dot: usize,
    gap: usize,
    dpr: usize,
) -> Option<String> {
    if config.grid.is_empty() {
        return None;
    }
    let (rows, columns) = (config.grid.rows.len(), config.grid.columns.len());
    // Sizes are configured in points, which is the unit the user is really
    // choosing; the terminal scales into a box measured the same way.
    let (dot, gap) = (dot * dpr, gap * dpr);
    let step = dot + gap;
    let (art_w, art_h) = (columns * step - gap, rows * step - gap);

    let (img_w, img_h) = (cells * cell.0 * dpr, cell.1 * dpr);
    if art_w > img_w || art_h > img_h {
        return None;
    }

    let states = config.states();
    let stride = 1 + img_w * 4;
    let mut raw = vec![0u8; stride * img_h]; // transparent: the terminal's own
                                             // background shows through
    let (ox, oy) = ((img_w - art_w) / 2, (img_h - art_h) / 2);
    for r in 0..rows {
        for c in 0..columns {
            let Some(index) = config
                .views
                .iter()
                .position(|view| view.at == Some((r, c)))
            else {
                continue; // a cell no view claims is left blank, not drawn
            };
            let colour = rgba(states[index], config.views[index].name == current);
            for dy in 0..dot {
                let base = (oy + r * step + dy) * stride + 1 + (ox + c * step) * 4;
                for dx in 0..dot {
                    raw[base + dx * 4..base + dx * 4 + 4].copy_from_slice(&colour);
                }
            }
        }
    }

    Some(escape_for(&encode_png(&raw, img_w, img_h), cells))
}

/// The placement escape: an exact cell extent, cursor pinned, quiet.
///
/// `q=2` suppresses the terminal's replies, which would otherwise land in the
/// shell's input stream and be typed as text.
fn escape_for(png: &[u8], cells: usize) -> String {
    let payload = base64(png);
    let mut out = String::with_capacity(payload.len() + 64);
    let mut rest = payload.as_str();
    let mut first = true;
    while !rest.is_empty() {
        // The protocol caps a chunk at 4096 bytes.
        let take = rest.len().min(4096);
        let (head, tail) = rest.split_at(take);
        let more = u8::from(!tail.is_empty());
        if first {
            out.push_str(&format!(
                "\x1b_Ga=T,f=100,c={cells},r=1,C=1,q=2,m={more};{head}\x1b\\"
            ));
            first = false;
        } else {
            out.push_str(&format!("\x1b_Gm={more};{head}\x1b\\"));
        }
        rest = tail;
    }
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, entry) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *entry = c;
    }
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = table[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in data {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn zlib_deflate(raw: &[u8]) -> Vec<u8> {
    let mut bits = BitWriter::new();
    bits.push(1, 1); // final block
    bits.push(1, 2); // fixed Huffman

    let mut i = 0;
    while i < raw.len() {
        // Two distances matter for this image. Distance 1 covers a run of
        // identical bytes; distance 4 covers a run of identical RGBA *pixels*,
        // which is what a block of flat colour actually is. Without the
        // second, a transparent background costs a byte every four pixels.
        let mut best = (0usize, 1u32);
        for distance in [4u32, 1] {
            let d = distance as usize;
            if i < d {
                continue;
            }
            let mut run = 0;
            while i + run < raw.len() && raw[i + run] == raw[i + run - d] && run < 258 {
                run += 1;
            }
            if run > best.0 {
                best = (run, distance);
            }
        }
        if best.0 >= 3 {
            emit_match(&mut bits, best.0, best.1);
            i += best.0;
        } else {
            emit_literal(&mut bits, raw[i]);
            i += 1;
        }
    }
    emit_literal_code(&mut bits, 256); // end of block

    let mut out = vec![0x78, 0x01];
    out.extend_from_slice(&bits.finish());
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

struct BitWriter {
    out: Vec<u8>,
    bit: u32,
    acc: u32,
}

impl BitWriter {
    fn new() -> Self {
        BitWriter { out: Vec::new(), bit: 0, acc: 0 }
    }
    /// Extra bits and headers: least significant bit first.
    fn push(&mut self, value: u32, count: u32) {
        for k in 0..count {
            self.acc |= ((value >> k) & 1) << self.bit;
            self.bit += 1;
            if self.bit == 8 {
                self.out.push(self.acc as u8);
                self.acc = 0;
                self.bit = 0;
            }
        }
    }
    /// Huffman codes: most significant bit first.
    fn push_code(&mut self, code: u32, count: u32) {
        for k in (0..count).rev() {
            self.acc |= ((code >> k) & 1) << self.bit;
            self.bit += 1;
            if self.bit == 8 {
                self.out.push(self.acc as u8);
                self.acc = 0;
                self.bit = 0;
            }
        }
    }
    fn finish(mut self) -> Vec<u8> {
        if self.bit > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

fn emit_literal(bits: &mut BitWriter, byte: u8) {
    emit_literal_code(bits, byte as u32);
}

fn emit_literal_code(bits: &mut BitWriter, symbol: u32) {
    match symbol {
        0..=143 => bits.push_code(0b0011_0000 + symbol, 8),
        144..=255 => bits.push_code(0b1_1001_0000 + (symbol - 144), 9),
        256..=279 => bits.push_code(symbol - 256, 7),
        _ => bits.push_code(0b1100_0000 + (symbol - 280), 8),
    }
}

fn emit_match(bits: &mut BitWriter, length: usize, distance: u32) {
    // RFC 1951 length codes 257..285, with their extra-bit widths and bases.
    const LENGTHS: [(u32, u32, u32); 29] = [
        (257, 0, 3), (258, 0, 4), (259, 0, 5), (260, 0, 6), (261, 0, 7),
        (262, 0, 8), (263, 0, 9), (264, 0, 10), (265, 1, 11), (266, 1, 13),
        (267, 1, 15), (268, 1, 17), (269, 2, 19), (270, 2, 23), (271, 2, 27),
        (272, 2, 31), (273, 3, 35), (274, 3, 43), (275, 3, 51), (276, 3, 59),
        (277, 4, 67), (278, 4, 83), (279, 4, 99), (280, 4, 115), (281, 5, 131),
        (282, 5, 163), (283, 5, 195), (284, 5, 227), (285, 0, 258),
    ];
    let mut chosen = LENGTHS[0];
    for entry in LENGTHS {
        if length as u32 >= entry.2 {
            chosen = entry;
        }
    }
    emit_literal_code(bits, chosen.0);
    if chosen.1 > 0 {
        bits.push(length as u32 - chosen.2, chosen.1);
    }
    // Distance codes are 5 bits, MSB first; code 0 is a distance of 1.
    bits.push_code(distance - 1, 5);
}

fn chunk(out: &mut Vec<u8>, tag: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    let mut with_tag = Vec::with_capacity(4 + body.len());
    with_tag.extend_from_slice(tag);
    with_tag.extend_from_slice(body);
    out.extend_from_slice(&with_tag);
    out.extend_from_slice(&crc32(&with_tag).to_be_bytes());
}

fn encode_png(raw: &[u8], img_w: usize, img_h: usize) -> Vec<u8> {
    let mut png = Vec::with_capacity(raw.len() / 8 + 128);
    png.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(img_w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(img_h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", &zlib_deflate(raw));
    chunk(&mut png, b"IEND", &[]);
    png
}

fn base64(data: &[u8]) -> String {
    const SET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for group in data.chunks(3) {
        let b = [group[0], *group.get(1).unwrap_or(&0), *group.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(SET[(n >> 18) as usize & 63] as char);
        out.push(SET[(n >> 12) as usize & 63] as char);
        out.push(if group.len() > 1 { SET[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if group.len() > 2 { SET[n as usize & 63] as char } else { '=' });
    }
    out
}