use std::env;
use std::fs;
use std::path::PathBuf;

const ICON_SIZES: [u32; 4] = [16, 32, 48, 256];
const INK: [u8; 4] = [46, 43, 38, 255];
const PANEL: [u8; 4] = [244, 239, 228, 255];
const CELL: [u8; 4] = [200, 68, 60, 255];
const RT_ICON: u32 = 3;
const RT_GROUP_ICON: u32 = 14;
const LANG_EN_US: u16 = 0x0409;

/// Embed the app icon as a Win32 resource. The icon itself is generated from the same
/// 16x16 pixel map the runtime uses, so the exe, window, title bar and tray cannot drift.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/anybattery-icon.pixels");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let source = fs::read_to_string(manifest.join("assets/anybattery-icon.pixels"))
        .expect("read assets/anybattery-icon.pixels");
    let grid = parse_pixel_map(&source);
    let images: Vec<Vec<u8>> = ICON_SIZES.iter().map(|size| icon_dib(*size, &grid)).collect();

    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let ico = build_ico(&images);
    fs::write(out.join("AnyBattery.ico"), &ico).expect("write generated ico");
    let res = build_resource(&images);
    let res_path = out.join("AnyBattery.res");
    fs::write(&res_path, &res).expect("write generated resource");
    println!("cargo:rustc-link-arg-bins={}", res_path.display());
}

fn parse_pixel_map(source: &str) -> Vec<Vec<char>> {
    let rows: Vec<Vec<char>> = source
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.chars().collect())
        .collect();
    assert_eq!(rows.len(), 16, "icon map must have exactly 16 rows");
    for (y, row) in rows.iter().enumerate() {
        assert_eq!(row.len(), 16, "icon map row {} must have exactly 16 columns", y);
        for (x, ch) in row.iter().enumerate() {
            assert!(
                matches!(*ch, '.' | '#' | 'w' | '1' | '2' | '3'),
                "unknown icon pixel {:?} at {},{}",
                ch,
                x,
                y
            );
        }
    }
    rows
}

fn pixel(grid: &[Vec<char>], x: u32, y: u32) -> [u8; 4] {
    match grid[y as usize][x as usize] {
        '.' => [0, 0, 0, 0],
        '#' => INK,
        'w' => PANEL,
        '1' | '2' | '3' => CELL,
        _ => unreachable!(),
    }
}

/// 32-bit bottom-up DIB plus its 1-bit transparency mask, as stored in an .ico image.
fn icon_dib(size: u32, grid: &[Vec<char>]) -> Vec<u8> {
    let xor_len = (size * size * 4) as usize;
    let mask_stride = size.div_ceil(32) * 4;
    let mask_len = (mask_stride * size) as usize;
    let mut out = Vec::with_capacity(40 + xor_len + mask_len);

    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&(size as i32).to_le_bytes());
    out.extend_from_slice(&((size * 2) as i32).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&((xor_len + mask_len) as u32).to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());

    for y in (0..size).rev() {
        for x in 0..size {
            let sx = x * 16 / size;
            let sy = y * 16 / size;
            let [r, g, b, a] = pixel(grid, sx, sy);
            out.extend_from_slice(&[b, g, r, a]);
        }
    }

    let mask_start = out.len();
    out.resize(mask_start + mask_len, 0);
    for y in 0..size {
        for x in 0..size {
            let sx = x * 16 / size;
            let sy = y * 16 / size;
            if pixel(grid, sx, sy)[3] == 0 {
                let row = (size - 1 - y) as usize;
                let index = mask_start + row * mask_stride as usize + (x / 8) as usize;
                out[index] |= 0x80 >> (x % 8);
            }
        }
    }
    out
}

fn build_ico(images: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&(images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (index, image) in images.iter().enumerate() {
        let size = ICON_SIZES[index];
        out.push(if size >= 256 { 0 } else { size as u8 });
        out.push(if size >= 256 { 0 } else { size as u8 });
        out.push(0);
        out.push(0);
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&32u16.to_le_bytes());
        out.extend_from_slice(&(image.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += image.len() as u32;
    }
    for image in images {
        out.extend_from_slice(image);
    }
    out
}

fn build_resource(images: &[Vec<u8>]) -> Vec<u8> {
    // .res files open with a null resource header (size 0, header size 32); lld-link uses
    // that first header to recognise the file as a resource script output.
    let mut out = Vec::with_capacity(32 + images.iter().map(Vec::len).sum::<usize>() + 128);
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&32u32.to_le_bytes());
    out.extend_from_slice(&0x0000FFFFu32.to_le_bytes());
    out.extend_from_slice(&0x0000FFFFu32.to_le_bytes());
    out.resize(32, 0);
    for (index, image) in images.iter().enumerate() {
        append_resource_entry(&mut out, RT_ICON, index as u16 + 1, 0x1010, image);
    }

    let mut group = Vec::new();
    group.extend_from_slice(&0u16.to_le_bytes());
    group.extend_from_slice(&1u16.to_le_bytes());
    group.extend_from_slice(&(images.len() as u16).to_le_bytes());
    for (index, image) in images.iter().enumerate() {
        let size = ICON_SIZES[index];
        group.push(if size >= 256 { 0 } else { size as u8 });
        group.push(if size >= 256 { 0 } else { size as u8 });
        group.push(0);
        group.push(0);
        group.extend_from_slice(&1u16.to_le_bytes());
        group.extend_from_slice(&32u16.to_le_bytes());
        group.extend_from_slice(&(image.len() as u32).to_le_bytes());
        group.extend_from_slice(&(index as u16 + 1).to_le_bytes());
    }
    append_resource_entry(&mut out, RT_GROUP_ICON, 1, 0x1030, &group);
    out
}

fn append_resource_entry(out: &mut Vec<u8>, kind: u32, name: u16, memory_flags: u16, data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&32u32.to_le_bytes());
    out.extend_from_slice(&(0xFFFFu32 | (kind << 16)).to_le_bytes());
    out.extend_from_slice(&(0xFFFFu32 | ((name as u32) << 16)).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&memory_flags.to_le_bytes());
    out.extend_from_slice(&LANG_EN_US.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(data);
    // Each resource block starts on a 4-byte boundary; lld-link rejects a short final stream.
    out.resize((out.len() + 3) & !3, 0);
}
