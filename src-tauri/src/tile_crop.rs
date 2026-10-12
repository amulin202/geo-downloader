//! Bounded-memory boundary clipping for an XYZ image tile directory.
use crate::{
    merger::{PolygonPoint, TileSource},
    pause_control::PauseControl,
};
use image::{DynamicImage, ImageFormat, Rgba};
use std::{
    collections::HashMap,
    io::{BufWriter, Read, Write},
    path::Path,
    sync::Arc,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct Ring {
    points: Vec<(f64, f64)>,
    bounds: (f64, f64, f64, f64),
}

struct TileClipper {
    rings: Vec<Ring>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Coverage {
    Empty,
    Full,
    Partial,
}

impl TileClipper {
    fn new(polygons: &[Vec<PolygonPoint>], zoom: u8) -> Result<Self, String> {
        if zoom > 24 {
            return Err("裁剪层级超出支持范围".into());
        }
        let scale = (1u64 << zoom) as f64;
        let mut rings = Vec::new();
        for polygon in polygons.iter().filter(|ring| ring.len() >= 3) {
            let mut points = Vec::with_capacity(polygon.len());
            let mut bounds = (
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            );
            for point in polygon {
                if !point.lat.is_finite()
                    || !point.lng.is_finite()
                    || point.lat.abs() > 90.0
                    || point.lng.abs() > 180.0
                {
                    return Err("裁剪边界含无效经纬度".into());
                }
                // Coordinates are in the same Web Mercator tile grid as the imagery.
                let x = (point.lng + 180.0) / 360.0 * scale;
                let y = (1.0
                    - crate::merger::mercator_y(point.lat.clamp(-85.05112878, 85.05112878))
                        / std::f64::consts::PI)
                    * 0.5
                    * scale;
                bounds.0 = bounds.0.min(x);
                bounds.1 = bounds.1.min(y);
                bounds.2 = bounds.2.max(x);
                bounds.3 = bounds.3.max(y);
                points.push((x, y));
            }
            if bounds.2 > bounds.0 && bounds.3 > bounds.1 {
                rings.push(Ring { points, bounds });
            }
        }
        if rings.is_empty() {
            return Err("没有有效的裁剪边界".into());
        }
        Ok(Self { rings })
    }

    fn coverage(&self, x: u32, y: u32) -> Coverage {
        let (left, top) = (x as f64, y as f64);
        let (cx, cy) = (left + 0.5, top + 0.5);
        let mut partial = false;
        for ring in &self.rings {
            let (min_x, min_y, max_x, max_y) = ring.bounds;
            if max_x <= left || min_x >= left + 1.0 || max_y <= top || min_y >= top + 1.0 {
                continue;
            }
            let mut inside = false;
            let mut boundary = false;
            for index in 0..ring.points.len() {
                let (ax, ay) = ring.points[index];
                let (bx, by) = ring.points[(index + 1) % ring.points.len()];
                if (ay > cy) != (by > cy) && cx < (bx - ax) * (cy - ay) / (by - ay) + ax {
                    inside = !inside;
                }
                // A bounding-box overlap is conservative: false positives merely use the mask path.
                if ax.max(bx) > left
                    && ax.min(bx) < left + 1.0
                    && ay.max(by) > top
                    && ay.min(by) < top + 1.0
                {
                    boundary = true;
                }
            }
            if inside && !boundary {
                return Coverage::Full;
            }
            partial |= boundary;
        }
        if partial {
            Coverage::Partial
        } else {
            Coverage::Empty
        }
    }

    fn write_tile(
        &self,
        source: &TileSource,
        x: u32,
        y: u32,
        destination: &Path,
    ) -> Result<Option<u64>, String> {
        let coverage = self.coverage(x, y);
        if coverage == Coverage::Empty {
            return Ok(None);
        }
        let parent = destination.parent().ok_or("裁剪瓦片路径无效")?;
        std::fs::create_dir_all(parent).map_err(|e| format!("创建裁剪目录失败: {e}"))?;
        let partial = destination.with_extension("crop-part");
        let operation = (|| -> Result<Option<u64>, String> {
            if coverage == Coverage::Full && is_png(source)? {
                source
                    .copy_to(&partial)
                    .map_err(|e| format!("复制裁剪瓦片失败: {e}"))?;
            } else {
                let bytes = source
                    .bytes()
                    .map_err(|e| format!("读取裁剪瓦片失败: {e}"))?;
                let mut image = image::load_from_memory(&bytes)
                    .map_err(|e| format!("解码裁剪瓦片失败: {e}"))?
                    .to_rgba8();
                drop(bytes);
                if coverage == Coverage::Partial {
                    let (width, height) = image.dimensions();
                    let mut has_pixels = false;
                    for row in 0..height {
                        let scan_y = y as f64 + (row as f64 + 0.5) / height as f64;
                        let mut covered = vec![false; width as usize];
                        // Rings represent separate selected areas; combine their coverage by union.
                        for ring in &self.rings {
                            if scan_y < ring.bounds.1 || scan_y >= ring.bounds.3 {
                                continue;
                            }
                            let mut crossings = Vec::new();
                            for index in 0..ring.points.len() {
                                let (ax, ay) = ring.points[index];
                                let (bx, by) = ring.points[(index + 1) % ring.points.len()];
                                if (ay > scan_y) != (by > scan_y) {
                                    crossings.push(ax + (bx - ax) * (scan_y - ay) / (by - ay));
                                }
                            }
                            crossings.sort_by(f64::total_cmp);
                            for span in crossings.chunks_exact(2) {
                                let start = ((span[0] - x as f64) * width as f64 - 0.5)
                                    .ceil()
                                    .clamp(0.0, width as f64)
                                    as usize;
                                let end = ((span[1] - x as f64) * width as f64 - 0.5)
                                    .ceil()
                                    .clamp(0.0, width as f64)
                                    as usize;
                                if start < end {
                                    covered[start..end].fill(true);
                                }
                            }
                        }
                        for (column, keep) in covered.into_iter().enumerate() {
                            if keep {
                                has_pixels = true;
                            } else {
                                image.put_pixel(column as u32, row, Rgba([0, 0, 0, 0]));
                            }
                        }
                    }
                    if !has_pixels {
                        return Ok(None);
                    }
                }
                let file = std::fs::File::create(&partial)
                    .map_err(|e| format!("创建裁剪瓦片失败: {e}"))?;
                let mut writer = BufWriter::with_capacity(64 * 1024, file);
                DynamicImage::ImageRgba8(image)
                    .write_to(&mut writer, ImageFormat::Png)
                    .map_err(|e| format!("编码裁剪瓦片失败: {e}"))?;
                writer
                    .flush()
                    .map_err(|e| format!("写入裁剪瓦片失败: {e}"))?;
            }
            std::fs::rename(&partial, destination).map_err(|e| format!("保存裁剪瓦片失败: {e}"))?;
            Ok(Some(
                std::fs::metadata(destination)
                    .map_err(|e| e.to_string())?
                    .len(),
            ))
        })();
        if operation.is_err() {
            let _ = std::fs::remove_file(&partial);
        }
        operation
    }
}

fn is_png(source: &TileSource) -> Result<bool, String> {
    let mut header = [0; 8];
    let length = match source {
        TileSource::Path(path) => std::fs::File::open(path)
            .and_then(|mut file| file.read(&mut header))
            .map_err(|e| format!("读取瓦片格式失败: {e}"))?,
        TileSource::Bytes(bytes) => {
            let length = bytes.len().min(8);
            header[..length].copy_from_slice(&bytes[..length]);
            length
        }
    };
    Ok(length == 8 && header == [137, 80, 78, 71, 13, 10, 26, 10])
}

#[derive(Debug, Default)]
pub struct CropExport {
    pub bytes: u64,
    pub kept: usize,
    pub skipped: usize,
}

pub async fn export_cropped_tiles(
    tiles: &HashMap<(u32, u32), TileSource>,
    polygons: &[Vec<PolygonPoint>],
    zoom: u8,
    directory: &Path,
    cancellation: &CancellationToken,
    pause: &PauseControl,
    mut progress: impl FnMut(usize, usize),
) -> Result<CropExport, String> {
    let clipper = Arc::new(TileClipper::new(polygons, zoom)?);
    let coordinates: Vec<_> = tiles.keys().copied().collect();
    let mut result = CropExport::default();
    for chunk in coordinates.chunks(64) {
        tokio::select! { biased;
            _ = cancellation.cancelled() => return Err("任务已取消".into()),
            _ = pause.wait_if_paused() => {},
        }
        let batch: Vec<_> = chunk
            .iter()
            .map(|&coordinate| (coordinate, tiles[&coordinate].clone()))
            .collect();
        let clipper = clipper.clone();
        let output = directory.join(zoom.to_string());
        let token = cancellation.clone();
        let counts = tokio::task::spawn_blocking(move || {
            let mut counts = CropExport::default();
            for ((x, y), source) in batch {
                if token.is_cancelled() {
                    return Err("任务已取消".to_string());
                }
                match clipper.write_tile(
                    &source,
                    x,
                    y,
                    &output.join(x.to_string()).join(format!("{y}.png")),
                )? {
                    Some(bytes) => {
                        counts.bytes += bytes;
                        counts.kept += 1;
                    }
                    None => counts.skipped += 1,
                }
            }
            Ok(counts)
        })
        .await
        .map_err(|e| format!("裁剪瓦片失败: {e}"))??;
        if cancellation.is_cancelled() {
            return Err("任务已取消".into());
        }
        result.bytes += counts.bytes;
        result.kept += counts.kept;
        result.skipped += counts.skipped;
        progress(result.kept + result.skipped, coordinates.len());
    }
    if result.kept == 0 {
        return Err("边界内没有可导出的瓦片像素，请提高层级或扩大选区".into());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GenericImageView, RgbaImage};

    fn point(x: f64, y: f64, zoom: u8) -> PolygonPoint {
        let scale = (1u64 << zoom) as f64;
        PolygonPoint {
            lng: x / scale * 360.0 - 180.0,
            lat: (std::f64::consts::PI * (1.0 - 2.0 * y / scale))
                .sinh()
                .atan()
                .to_degrees(),
        }
    }
    fn rectangle(left: f64, top: f64, right: f64, bottom: f64, zoom: u8) -> Vec<PolygonPoint> {
        vec![
            point(left, top, zoom),
            point(right, top, zoom),
            point(right, bottom, zoom),
            point(left, bottom, zoom),
        ]
    }
    fn source(directory: &Path, size: u32) -> TileSource {
        let path = directory.join(format!("input-{size}.png"));
        RgbaImage::from_pixel(size, size, Rgba([10, 20, 30, 128]))
            .save(&path)
            .unwrap();
        TileSource::from_path(path)
    }

    #[test]
    fn clipping_uses_mercator_pixels_and_keeps_dimensions_and_existing_alpha() {
        let dir = tempfile::tempdir().unwrap();
        for size in [256, 512] {
            // z5/y5 is at a high latitude, where linear latitude mapping is incorrect.
            let clipper = TileClipper::new(&[rectangle(4.25, 5.25, 4.75, 5.75, 5)], 5).unwrap();
            let input = source(dir.path(), size);
            let original = input.bytes().unwrap().into_owned();
            let output = dir.path().join(format!("output-{size}.png"));
            assert!(clipper.write_tile(&input, 4, 5, &output).unwrap().is_some());
            let image = image::open(output).unwrap().to_rgba8();
            assert_eq!(image.dimensions(), (size, size));
            for (x, y, pixel) in image.enumerate_pixels() {
                let inside = x >= size / 4 && x < size * 3 / 4 && y >= size / 4 && y < size * 3 / 4;
                assert_eq!(
                    pixel.0,
                    if inside {
                        [10, 20, 30, 128]
                    } else {
                        [0, 0, 0, 0]
                    },
                    "{size}: {x}/{y}"
                );
            }
            assert_eq!(input.bytes().unwrap().as_ref(), original);
        }
    }

    #[test]
    fn polygon_union_keeps_overlaps_and_concave_regions() {
        let dir = tempfile::tempdir().unwrap();
        let clipper = TileClipper::new(
            &[
                rectangle(4.0, 5.0, 4.75, 6.0, 5),
                rectangle(4.25, 5.0, 5.0, 6.0, 5),
            ],
            5,
        )
        .unwrap();
        let input = source(dir.path(), 8);
        let output = dir.path().join("union.png");
        clipper.write_tile(&input, 4, 5, &output).unwrap();
        assert!(image::open(output)
            .unwrap()
            .to_rgba8()
            .pixels()
            .all(|pixel| pixel.0 == [10, 20, 30, 128]));
        let ring = [
            (4., 5.),
            (5., 5.),
            (5., 5.5),
            (4.5, 5.5),
            (4.5, 6.),
            (4., 6.),
        ]
        .map(|(x, y)| point(x, y, 5))
        .to_vec();
        let clipper = TileClipper::new(&[ring], 5).unwrap();
        let output = dir.path().join("concave.png");
        clipper.write_tile(&input, 4, 5, &output).unwrap();
        let image = image::open(output).unwrap().to_rgba8();
        assert_eq!(image.get_pixel(6, 6).0, [0, 0, 0, 0]);
        assert_eq!(image.get_pixel(1, 6).0, [10, 20, 30, 128]);
        assert_eq!(image.get_pixel(6, 1).0, [10, 20, 30, 128]);
    }

    #[tokio::test]
    async fn xyz_export_omits_outside_tiles_and_handles_multiple_zoom_levels() {
        let dir = tempfile::tempdir().unwrap();
        let input = source(dir.path(), 8);
        let polygons = vec![rectangle(4.5, 5.0, 5.5, 6.0, 5)];
        let output = dir.path().join("xyz");
        for zoom in [5, 6] {
            let multiplier = if zoom == 5 { 1 } else { 2 };
            let tiles: HashMap<_, _> = (3 * multiplier..7 * multiplier)
                .flat_map(|x| (5 * multiplier..6 * multiplier).map(move |y| (x, y)))
                .map(|coord| (coord, input.clone()))
                .collect();
            let result = export_cropped_tiles(
                &tiles,
                &polygons,
                zoom,
                &output,
                &CancellationToken::new(),
                &PauseControl::default(),
                |_, _| {},
            )
            .await
            .unwrap();
            assert_eq!(result.kept, if zoom == 5 { 2 } else { 4 });
            assert_eq!(result.kept + result.skipped, tiles.len());
        }
        assert!(!output.join("5/3/5.png").exists());
        let left = image::open(output.join("5/4/5.png")).unwrap().to_rgba8();
        let right = image::open(output.join("5/5/5.png")).unwrap().to_rgba8();
        for y in 0..8 {
            assert_eq!(left.get_pixel(3, y).0, [0, 0, 0, 0]);
            assert_eq!(left.get_pixel(4, y).0, [10, 20, 30, 128]);
            assert_eq!(right.get_pixel(3, y).0, [10, 20, 30, 128]);
            assert_eq!(right.get_pixel(4, y).0, [0, 0, 0, 0]);
        }
        assert!(input.bytes().is_ok());
    }

    #[tokio::test]
    async fn crop_runs_after_three_overlays_and_never_mutates_inputs() {
        let dir = tempfile::tempdir().unwrap();
        let original = source(dir.path(), 8);
        let mut tiles = HashMap::from([((4, 5), original.clone())]);
        let mut layers = Vec::new();
        for (index, color) in [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 128]]
            .into_iter()
            .enumerate()
        {
            let path = dir.path().join(format!("overlay-{index}.png"));
            RgbaImage::from_pixel(8, 8, Rgba(color))
                .save(&path)
                .unwrap();
            layers.push(HashMap::from([((4, 5), TileSource::from_path(path))]));
        }
        crate::overlay::composite_tiles_to_disk(
            &mut tiles,
            &layers,
            &dir.path().join("composite"),
            &CancellationToken::new(),
            &PauseControl::default(),
            |_, _| {},
        )
        .await
        .unwrap();
        let before = image::load_from_memory(&tiles[&(4, 5)].bytes().unwrap())
            .unwrap()
            .to_rgba8();
        let polygons = vec![rectangle(4.5, 5., 5., 6., 5)];
        let output = dir.path().join("cropped");
        for _ in 0..2 {
            export_cropped_tiles(
                &tiles,
                &polygons,
                5,
                &output,
                &CancellationToken::new(),
                &PauseControl::default(),
                |_, _| {},
            )
            .await
            .unwrap();
            let after = image::open(output.join("5/4/5.png")).unwrap().to_rgba8();
            assert_eq!(after.get_pixel(6, 6), before.get_pixel(6, 6));
            assert_eq!(after.get_pixel(1, 6).0, [0, 0, 0, 0]);
        }
        assert!(original.bytes().is_ok());
    }

    #[tokio::test]
    async fn cancellation_and_pause_keep_originals_available_for_retry() {
        let dir = tempfile::tempdir().unwrap();
        let input = source(dir.path(), 8);
        let tiles: HashMap<_, _> = (0..150).map(|x| ((x, 0), input.clone())).collect();
        let polygons = vec![rectangle(0., 0., 150., 1., 8)];
        let token = CancellationToken::new();
        let pause = PauseControl::default();
        let output = dir.path().join("out");
        let error =
            export_cropped_tiles(&tiles, &polygons, 8, &output, &token, &pause, |done, _| {
                assert_eq!(done, 64);
                token.cancel();
            })
            .await
            .unwrap_err();
        assert!(error.contains("取消"));
        let result = export_cropped_tiles(
            &tiles,
            &polygons,
            8,
            &output,
            &CancellationToken::new(),
            &pause,
            |_, _| {},
        )
        .await
        .unwrap();
        assert_eq!(result.kept, 150);
        pause.flag.store(true, std::sync::atomic::Ordering::Relaxed);
        let token = CancellationToken::new();
        let signal = token.clone();
        let worker = tokio::spawn(async move {
            tokio::task::yield_now().await;
            signal.cancel();
        });
        let error = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            export_cropped_tiles(
                &tiles,
                &polygons,
                8,
                &dir.path().join("paused"),
                &token,
                &pause,
                |_, _| panic!("paused"),
            ),
        )
        .await
        .unwrap()
        .unwrap_err();
        worker.await.unwrap();
        assert!(error.contains("取消"));
        assert!(input.bytes().is_ok());
    }

    #[test]
    fn full_jpeg_is_converted_to_png_and_disk_failure_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.jpg");
        image::RgbImage::from_pixel(256, 256, image::Rgb([30, 40, 50]))
            .save(&path)
            .unwrap();
        let source = TileSource::from_path(path);
        let clipper = TileClipper::new(&[rectangle(0., 0., 2., 2., 2)], 2).unwrap();
        let output = dir.path().join("full.png");
        clipper.write_tile(&source, 0, 0, &output).unwrap();
        assert_eq!(image::open(output).unwrap().dimensions(), (256, 256));
        std::fs::write(dir.path().join("blocked"), b"file").unwrap();
        assert!(clipper
            .write_tile(&source, 0, 0, &dir.path().join("blocked/result.png"))
            .is_err());
        assert!(TileClipper::new(&[], 2).is_err());
    }
}
