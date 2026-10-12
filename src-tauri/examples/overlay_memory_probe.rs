// Bounded diagnostic using synthetic texture; not a replay of the user's task.
// Uses the same image decoding, alpha overlay and PNG encoding as commands.rs.
use app_lib::{merger::TileSource, overlay::composite_tiles_to_disk, pause_control::PauseControl};
use image::{DynamicImage, ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};
use std::collections::HashMap;
use std::io::Cursor;
use tokio_util::sync::CancellationToken;

#[cfg(windows)]
fn private_memory_bytes() -> usize {
    #[repr(C)]
    struct Counters {
        cb: u32,
        page_faults: u32,
        peak_working_set: usize,
        working_set: usize,
        peak_paged_pool: usize,
        paged_pool: usize,
        peak_nonpaged_pool: usize,
        nonpaged_pool: usize,
        pagefile: usize,
        peak_pagefile: usize,
        private: usize,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
    }
    #[link(name = "psapi")]
    extern "system" {
        fn GetProcessMemoryInfo(
            process: *mut std::ffi::c_void,
            counters: *mut Counters,
            size: u32,
        ) -> i32;
    }
    unsafe {
        let mut counters: Counters = std::mem::zeroed();
        counters.cb = std::mem::size_of::<Counters>() as u32;
        assert_ne!(
            GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb),
            0
        );
        counters.private
    }
}

#[cfg(not(windows))]
fn private_memory_bytes() -> usize {
    0
}

fn encode(image: &DynamicImage, format: ImageFormat, initial_capacity: usize) -> Vec<u8> {
    let mut output = Vec::with_capacity(initial_capacity);
    image
        .write_to(&mut Cursor::new(&mut output), format)
        .unwrap();
    output
}

#[tokio::main]
async fn main() {
    let count = 336_660_u64;
    let mut seed = 0x1234_5678_u32;
    let mut texture = RgbImage::new(256, 256);
    for (x, y, pixel) in texture.enumerate_pixels_mut() {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let noise = seed % 47;
        *pixel = Rgb([
            ((x * 3 + y * 2 + noise) % 256) as u8,
            ((x + y * 5 + noise) % 256) as u8,
            ((x * 2 + y + noise) % 256) as u8,
        ]);
    }
    let base_jpeg = encode(&DynamicImage::ImageRgb8(texture), ImageFormat::Jpeg, 0);
    let mut base = image::load_from_memory(&base_jpeg).unwrap().to_rgba8();
    let mut annotation_bytes = 0;
    let directory = tempfile::tempdir().unwrap();
    let main_path = directory.path().join("main.jpg");
    std::fs::write(&main_path, &base_jpeg).unwrap();
    let mut layers: Vec<HashMap<(u32, u32), TileSource>> = Vec::new();
    for layer in 0..3 {
        let mut annotation = RgbaImage::new(256, 256);
        for (x, y, pixel) in annotation.enumerate_pixels_mut() {
            if (x + 7 * layer) % 61 < 2 || (y + 13 * layer) % 89 < 2 {
                *pixel = Rgba([240, 240 - layer as u8 * 60, 255, 180]);
            }
        }
        let bytes = encode(&DynamicImage::ImageRgba8(annotation), ImageFormat::Png, 0);
        annotation_bytes += bytes.len();
        let path = directory.path().join(format!("layer-{layer}.png"));
        std::fs::write(&path, &bytes).unwrap();
        layers.push(
            (0..512)
                .map(|x| ((x, 0), TileSource::from_path(path.clone())))
                .collect(),
        );
        let overlay = image::load_from_memory(&bytes).unwrap().to_rgba8();
        image::imageops::overlay(&mut base, &overlay, 0, 0);
    }
    let png = encode(
        &DynamicImage::ImageRgba8(base),
        ImageFormat::Png,
        base_jpeg.len(),
    );
    println!("synthetic_sample=true tile_size=256x256 overlays=3");
    println!(
        "base_jpeg_bytes={} three_annotation_png_bytes={}",
        base_jpeg.len(),
        annotation_bytes
    );
    println!(
        "composite_png_bytes={} composite_vec_capacity={}",
        png.len(),
        png.capacity()
    );
    if std::env::args().nth(1).as_deref() == Some("disk") {
        let baseline = private_memory_bytes();
        let mut tiles = (0..512)
            .map(|x| ((x, 0), TileSource::from_path(main_path.clone())))
            .collect();
        let output = directory.path().join("composited");
        let result = composite_tiles_to_disk(
            &mut tiles,
            &layers,
            &output,
            &CancellationToken::new(),
            &PauseControl::default(),
            |done, _| {
                if [128, 256, 512].contains(&done) {
                    println!(
                        "mode=disk retained_tiles={} private_mib={:.2} growth_mib={:.2}",
                        done,
                        private_memory_bytes() as f64 / 1024.0 / 1024.0,
                        private_memory_bytes().saturating_sub(baseline) as f64 / 1024.0 / 1024.0
                    );
                }
            },
        )
        .await
        .unwrap();
        assert_eq!(result, (512, 0));
        for source in tiles.values() {
            assert!(matches!(source, TileSource::Path(_)));
            assert_eq!(source.bytes().unwrap().as_ref(), png);
        }
        println!("verified_composites=512 retained_png_payload_bytes=0");
        return;
    }
    println!(
        "projected_png_payload_gib={:.2} projected_vec_capacity_gib={:.2}",
        png.len() as f64 * count as f64 / (1024_f64.powi(3)),
        png.capacity() as f64 * count as f64 / (1024_f64.powi(3))
    );
    let mut retained: Vec<Vec<u8>> = Vec::new();
    let baseline = private_memory_bytes();
    for i in 1..=512 {
        let mut tile_result = Vec::with_capacity(png.capacity());
        tile_result.extend_from_slice(&png);
        retained.push(tile_result);
        if [128, 256, 512].contains(&i) {
            let payload: usize = retained.iter().map(Vec::len).sum();
            let capacity: usize = retained.iter().map(Vec::capacity).sum();
            println!(
                "retained_tiles={} png_payload_mib={:.2} allocation_capacity_mib={:.2}",
                i,
                payload as f64 / 1024.0 / 1024.0,
                capacity as f64 / 1024.0 / 1024.0
            );
            println!(
                "mode=memory private_mib={:.2} growth_mib={:.2}",
                private_memory_bytes() as f64 / 1024.0 / 1024.0,
                private_memory_bytes().saturating_sub(baseline) as f64 / 1024.0 / 1024.0
            );
        }
    }
    // The picture data is synthetic, and these projections are not measured user RAM usage.
    std::hint::black_box(&retained);
}
