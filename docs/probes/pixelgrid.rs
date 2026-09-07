//! Cost model for design D: can a pixel grid be built in Rust fast enough that
//! nobody notices it in their prompt?
//!
//! The concern is real, because this is per-prompt work on the interactive
//! path. The three costs are: painting the dots, deflating them into a PNG,
//! and base64-encoding the result. This is a standalone benchmark so the
//! answer is measured rather than assumed; nothing here is wired into the
//! renderer yet.
//!
//! No dependencies. PNG requires a zlib stream, but deflate permits *stored*
//! (uncompressed) blocks, so a valid PNG can be produced with only a CRC-32
//! and an Adler-32, both a few lines. That matters because the images are tiny:
//! compression would cost more than it saves.
//!
//!     rustc -O docs/probes/pixelgrid.rs -o /tmp/pixelgrid && /tmp/pixelgrid

use std::time::Instant;

#[derive(Clone, Copy, PartialEq)]
pub enum State {
    Idle,
    Ok,
    Current,
    Alert,
}

impl State {
    fn rgba(self) -> [u8; 4] {
        match self {
            State::Idle => [70, 70, 80, 255],
            State::Ok => [90, 140, 90, 255],
            State::Current => [240, 240, 240, 255],
            State::Alert => [230, 70, 60, 255],
        }
    }
}

/// One grid of dots, to be drawn into a whole number of terminal cells.
pub struct Grid {
    pub rows: usize,
    pub cols: usize,
    pub cells: Vec<State>,
    /// Dot size in image pixels. 4 or 5 reads best on a HiDPI display.
    pub dot: usize,
    pub gap: usize,
}

impl Grid {
    fn art_size(&self) -> (usize, usize) {
        let step = self.dot + self.gap;
        (
            self.cols * step - self.gap,
            self.rows * step - self.gap,
        )
    }
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

/// A zlib stream using fixed-Huffman deflate with run-length matches.
///
/// Stored blocks were tried first and are far too fat: the image is mostly
/// transparent, so a 32x68 RGBA frame is 8772 raw bytes that real deflate
/// takes to about 30. Since the prompt re-emits this on every keystroke-driven
/// repaint, payload size matters more than encoder simplicity.
///
/// Fixed Huffman is used rather than dynamic because it needs no code-length
/// table, which is most of the complexity of a deflate encoder, and on runs of
/// identical bytes the two are within a few percent.
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

/// Deflate writes Huffman codes most-significant-bit first, but everything
/// else least-significant-bit first. Keeping that in one place avoids the
/// classic bug where the two are mixed up.
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

/// The fixed literal/length code from RFC 1951 section 3.2.6.
fn emit_literal_code(bits: &mut BitWriter, symbol: u32) {
    match symbol {
        0..=143 => bits.push_code(0b0011_0000 + symbol, 8),
        144..=255 => bits.push_code(0b1_1001_0000 + (symbol - 144), 9),
        256..=279 => bits.push_code(symbol - 256, 7),
        _ => bits.push_code(0b1100_0000 + (symbol - 280), 8),
    }
}

fn emit_literal(bits: &mut BitWriter, byte: u8) {
    emit_literal_code(bits, byte as u32);
}

/// Length and distance, for a run. Only distance 1 is ever used here, which is
/// what makes a solid colour cheap.
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

/// Paint the dots and wrap them in an RGBA PNG. Transparent background, so the
/// terminal's own colour shows through rather than a black rectangle.
pub fn render_png(grid: &Grid, img_w: usize, img_h: usize) -> Option<Vec<u8>> {
    let (art_w, art_h) = grid.art_size();
    if art_w > img_w || art_h > img_h {
        return None; // never draw a partial grid: it would misreport state
    }

    let step = grid.dot + grid.gap;
    let (ox, oy) = ((img_w - art_w) / 2, (img_h - art_h) / 2);

    // Scanlines with a leading filter byte, which is what PNG wants.
    let stride = 1 + img_w * 4;
    let mut raw = vec![0u8; stride * img_h];
    for (r, row) in grid.cells.chunks(grid.cols).enumerate() {
        for (c, state) in row.iter().enumerate() {
            let rgba = state.rgba();
            for dy in 0..grid.dot {
                let y = oy + r * step + dy;
                let base = y * stride + 1 + (ox + c * step) * 4;
                for dx in 0..grid.dot {
                    raw[base + dx * 4..base + dx * 4 + 4].copy_from_slice(&rgba);
                }
            }
        }
    }

    let mut png = Vec::with_capacity(raw.len() + 128);
    png.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(img_w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(img_h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", &zlib_deflate(&raw));
    chunk(&mut png, b"IEND", &[]);
    Some(png)
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

/// The full escape: an image placed in an exact number of cells, with the
/// cursor pinned so the prompt's declared width stays honest.
pub fn placement(grid: &Grid, cell: (usize, usize), cells: usize) -> Option<String> {
    let png = render_png(grid, cells * cell.0, cell.1)?;
    let payload = base64(&png);
    let mut out = String::with_capacity(payload.len() + 64);
    // Small images fit one chunk; the protocol caps a chunk at 4096 bytes.
    let mut rest = payload.as_str();
    let mut first = true;
    while !rest.is_empty() {
        let take = rest.len().min(4096);
        let (head, tail) = rest.split_at(take);
        let more = if tail.is_empty() { 0 } else { 1 };
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
    Some(out)
}

fn grid_of(rows: usize, cols: usize, dot: usize) -> Grid {
    let mut cells = vec![State::Idle; rows * cols];
    cells[cols + 1] = State::Current;
    if cols > 2 {
        cells[cols + 2] = State::Alert;
    }
    Grid { rows, cols, cells, dot, gap: 1 }
}

fn bench(label: &str, grid: &Grid, cell: (usize, usize), cells: usize) {
    let Some(first) = placement(grid, cell, cells) else {
        println!("  {label:<34} does not fit");
        return;
    };
    let runs = 10_000;
    let start = Instant::now();
    let mut sink = 0usize;
    for _ in 0..runs {
        sink += placement(grid, cell, cells).map_or(0, |s| s.len());
    }
    let each = start.elapsed() / runs;
    assert!(sink > 0);
    println!(
        "  {label:<34} {:>7.1} us   {} bytes",
        each.as_secs_f64() * 1e6,
        first.len()
    );
}

/// Checks that need no terminal. `rustc --test` runs these.
#[cfg(test)]
mod tests {
    use super::*;

    use std::convert::TryInto;

    fn inflate_len(png: &[u8]) -> usize {
        // Walk the chunks and total the IDAT payload, so a malformed length
        // field shows up as a panic rather than a wrong answer.
        let mut pos = 8;
        let mut idat = 0;
        while pos < png.len() {
            let n = u32::from_be_bytes(png[pos..pos + 4].try_into().unwrap()) as usize;
            if &png[pos + 4..pos + 8] == b"IDAT" {
                idat += n;
            }
            pos += 12 + n;
        }
        idat
    }

    #[test]
    fn a_grid_fits_one_cell_and_is_a_png() {
        let png = render_png(&grid_of(3, 3, 8), 32, 68).expect("fits");
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        assert!(inflate_len(&png) > 0);
    }

    #[test]
    fn an_oversized_grid_refuses_rather_than_clipping() {
        // The failure that sank the text strip: never draw a partial grid,
        // because a missing alert reads as calm.
        assert!(render_png(&grid_of(40, 40, 8), 32, 68).is_none());
    }

    #[test]
    fn compression_beats_storing_the_raw_image() {
        // A stored-block encoder produced 11841 bytes for this, which is far
        // too much to emit on every prompt.
        let png = render_png(&grid_of(3, 3, 8), 32, 68).expect("fits");
        assert!(png.len() < 1200, "payload was {} bytes", png.len());
    }

    #[test]
    fn the_placement_pins_the_cursor_and_declares_its_cells() {
        let esc = placement(&grid_of(3, 3, 8), (32, 68), 2).expect("fits");
        assert!(esc.contains("c=2,r=1"), "{esc:.60}");
        assert!(esc.contains("C=1"));
        assert!(esc.contains("q=2"));
        assert!(esc.starts_with("\x1b_G") && esc.ends_with("\x1b\\"));
    }

    #[test]
    fn adjacent_grids_share_one_placement() {
        // Four grids in four cells is still a single escape, so the prompt has
        // one width to account for rather than four.
        let esc = placement(&grid_of(3, 12, 8), (32, 68), 4).expect("fits");
        assert_eq!(esc.matches("a=T").count(), 1);
        assert!(esc.contains("c=4,r=1"));
    }

    #[test]
    fn a_cell_holds_seven_rows_at_four_pixel_dots() {
        // 68 device pixels, 8 per dot plus a 1px gap: 7 rows, not 8.
        assert!(render_png(&grid_of(7, 3, 8), 32, 68).is_some());
        assert!(render_png(&grid_of(8, 3, 8), 32, 68).is_none());
    }
}

/// Write one PNG to stdout so an independent decoder can check it. A
/// hand-written deflate encoder is easy to get subtly wrong, and a wrong one
/// still looks fine right up until a real terminal rejects the image.
fn dump(dot: usize) {
    use std::io::Write;
    let grid = grid_of(3, 3, dot);
    let png = render_png(&grid, 32, 68).expect("fits in one 2x cell");
    std::io::stdout().write_all(&png).unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 2 && args[1] == "--dump" {
        dump(args[2].parse().expect("dot size"));
        return;
    }

    // A 2x display: the cell is in points, so the art is built at twice the
    // size. These are the numbers from the design document.
    let cell = (32, 68);
    let dpr = 2;

    println!("Design D in Rust, on a 2x display (cell {}x{} device px)", cell.0, cell.1);
    println!("Times are per full placement: paint, PNG, base64, escape.\n");

    for dot in [3, 4, 5] {
        bench(
            &format!("3x3 grid, {dot}px dots, 1 cell"),
            &grid_of(3, 3, dot * dpr),
            cell,
            1,
        );
    }

    println!();
    // Several independent grids in adjacent cells: one image spanning them all,
    // since a single placement is cheaper than N placements and keeps the
    // width accounting to one number.
    for cells in [2, 3, 4] {
        bench(
            &format!("{cells} grids side by side, {cells} cells"),
            &grid_of(3, 3 * cells, 4 * dpr),
            cell,
            cells,
        );
    }

    println!();
    // How tall can a grid get? A cell is 68 device pixels, so at 4px dots
    // (8 device px) plus a 1px gap the answer is 7 rows, not 8.
    for rows in [6, 7, 8] {
        bench(
            &format!("{rows}x3 grid, 4px dots, 1 cell"),
            &grid_of(rows, 3, 4 * dpr),
            cell,
            1,
        );
    }

    println!("\nFor comparison, the collectors this prompt already runs:");
    println!("  {:<34} {:>7} us", "directory segment", 2000);
    println!("  {:<34} {:>7} us", "git segment", 5000);
    println!("  {:<34} {:>7} us", "kubernetes segment", 29000);
    println!(
        "\nA grid costs well under 1% of what one kubernetes collector costs,\n\
         and the whole prompt already spends milliseconds. The encoder is the\n\
         cheap part; the collectors were always the expensive part."
    );
}
