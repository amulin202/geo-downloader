//! 内存预算预警模块
//!
//! 在任务执行前预估峰值内存，超出预算时仅提示并给出可行建议。

use serde::Serialize;

/// 默认内存预算 (MB)
pub const DEFAULT_BUDGET_MB: u64 = 2048;

/// 每个瓦片的像素尺寸
const TILE_SIZE: u64 = 256;

/// 系统内存信息
#[derive(Debug, Clone, Serialize)]
pub struct SystemMemoryInfo {
    /// 物理内存总量 (MB)
    pub total_mb: u64,
    /// 可用物理内存 (MB)
    pub available_mb: u64,
}

/// 获取系统物理内存信息
#[cfg(target_os = "windows")]
pub fn get_system_memory() -> Option<SystemMemoryInfo> {
    use std::mem;

    #[repr(C)]
    struct MEMORYSTATUSEX {
        dw_length: u32,
        dw_memory_load: u32,
        ull_total_phys: u64,
        ull_avail_phys: u64,
        ull_total_page_file: u64,
        ull_avail_page_file: u64,
        ull_total_virtual: u64,
        ull_avail_virtual: u64,
        ull_avail_extended_virtual: u64,
    }

    extern "system" {
        fn GlobalMemoryStatusEx(lpBuffer: *mut MEMORYSTATUSEX) -> i32;
    }

    unsafe {
        let mut status: MEMORYSTATUSEX = mem::zeroed();
        status.dw_length = mem::size_of::<MEMORYSTATUSEX>() as u32;
        if GlobalMemoryStatusEx(&mut status) != 0 {
            Some(SystemMemoryInfo {
                total_mb: status.ull_total_phys / (1024 * 1024),
                available_mb: status.ull_avail_phys / (1024 * 1024),
            })
        } else {
            None
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn get_system_memory() -> Option<SystemMemoryInfo> {
    #[cfg(target_os = "linux")]
    {
        // 读取 /proc/meminfo
        let content = std::fs::read_to_string("/proc/meminfo").ok()?;
        let mut total_kb = 0u64;
        let mut available_kb = 0u64;
        for line in content.lines() {
            if line.starts_with("MemTotal:") {
                total_kb = line.split_whitespace().nth(1)?.parse().ok()?;
            } else if line.starts_with("MemAvailable:") {
                available_kb = line.split_whitespace().nth(1)?.parse().ok()?;
            }
        }
        if total_kb > 0 {
            return Some(SystemMemoryInfo {
                total_mb: total_kb / 1024,
                available_mb: available_kb / 1024,
            });
        }
        None
    }
    #[cfg(target_os = "macos")]
    {
        use std::mem;

        extern "C" {
            fn sysctl(
                name: *const i32, namelen: u32,
                oldp: *mut std::ffi::c_void, oldlenp: *mut usize,
                newp: *const std::ffi::c_void, newlen: usize,
            ) -> i32;
        }

        // CTL_HW=6, HW_MEMSIZE=24
        let mib: [i32; 2] = [6, 24];
        let mut memsize: u64 = 0;
        let mut len = mem::size_of::<u64>();
        let ret = unsafe {
            sysctl(
                mib.as_ptr(), 2,
                &mut memsize as *mut u64 as *mut std::ffi::c_void,
                &mut len,
                std::ptr::null(), 0,
            )
        };
        if ret == 0 && memsize > 0 {
            let total_mb = memsize / (1024 * 1024);
            // macOS 没有 MemAvailable 的直接概念，用 vm_statistics 近似
            // 简化处理：报告总量，可用设为 0（前端会处理）
            Some(SystemMemoryInfo {
                total_mb,
                available_mb: 0, // macOS 上精确可用内存需要 host_statistics64，此处简化
            })
        } else {
            None
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

/// 导出格式分类（用于内存估算）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatClass {
    /// 原始瓦片 / MBTiles / GeoPackage：逐瓦片写入，不分配整幅画布。
    TileByTile,
    /// 流式 GeoTIFF (strip-by-strip)
    StreamingTiff,
    /// 全量 GeoTIFF (in-memory)
    FullTiff,
    /// PNG / JPEG (in-memory encode)
    RasterImage,
}

/// 获取峰值倍率
///
/// | 格式        | 无裁剪 | 有裁剪 |
/// |------------|--------|--------|
/// | 流式 TIFF   | 0.05  | n/a    |
/// | 全量 TIFF   | 2.0   | 3.5    |
/// | PNG/JPEG   | 2.5   | 5.0    |
fn peak_multiplier(format: FormatClass, has_crop: bool) -> f64 {
    match (format, has_crop) {
        (FormatClass::TileByTile, _) => 0.0, // Uses a path/index working-set estimate below.
        (FormatClass::StreamingTiff, _) => 0.05,
        (FormatClass::FullTiff, false) => 2.0,
        (FormatClass::FullTiff, true) => 3.5,
        (FormatClass::RasterImage, false) => 2.5,
        (FormatClass::RasterImage, true) => 5.0,
    }
}

/// 内存预算检查结果
#[derive(Debug, Clone, Serialize)]
pub struct BudgetCheckResult {
    /// 预估峰值内存 (bytes)
    pub estimated_peak_bytes: u64,
    /// 内存预算 (bytes)
    pub budget_bytes: u64,
    /// 预估峰值是否在预算内；false 仅表示需要预警，不禁止下载或导出
    pub allowed: bool,
    /// 超预算时的建议列表
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<String>,
}

/// 检查内存预算
///
/// - `cols`, `rows`: 瓦片矩阵列数和行数
/// - `format`: 导出格式分类
/// - `has_crop`: 是否启用多边形裁剪
/// - `budget_mb`: 内存预警阈值 (MB)，0 表示不限
pub fn check_budget(
    cols: u32,
    rows: u32,
    format: FormatClass,
    has_crop: bool,
    budget_mb: u64,
) -> BudgetCheckResult {
    let budget_bytes = budget_mb.saturating_mul(1024 * 1024);

    // RGB 3 通道；裁剪时 RGBA 4 通道
    let channels: u64 = if has_crop { 4 } else { 3 };
    let pixel_bytes = (cols as u64)
        .checked_mul(rows as u64)
        .and_then(|v| v.checked_mul(TILE_SIZE))
        .and_then(|v| v.checked_mul(TILE_SIZE))
        .and_then(|v| v.checked_mul(channels))
        .unwrap_or(u64::MAX);
    let multiplier = peak_multiplier(format, has_crop);
    let estimated_peak = if format == FormatClass::TileByTile {
        // Bounded decoder/encoder buffers plus path maps for main + three overlays.
        // This is an approximate working set, not the size of the output files.
        128u64.saturating_mul(1024 * 1024)
            .saturating_add((cols as u64).saturating_mul(rows as u64).saturating_mul(1024))
    } else {
        (pixel_bytes as f64 * multiplier) as u64
    };

    if budget_mb == 0 || estimated_peak <= budget_bytes {
        return BudgetCheckResult {
            estimated_peak_bytes: estimated_peak,
            budget_bytes,
            allowed: true,
            suggestions: vec![],
        };
    }

    // 超预算 → 生成建议
    let mut suggestions = Vec::new();
    let peak_mb = estimated_peak / (1024 * 1024);

    // 建议 1: 缩小区域
    let safe_tiles = if format == FormatClass::TileByTile {
        budget_bytes.saturating_sub(128 * 1024 * 1024) / 1024
    } else {
        (budget_bytes as f64 / multiplier / (TILE_SIZE * TILE_SIZE * channels) as f64) as u64
    };
    suggestions.push(format!(
        "请缩小选区至约 {} 个瓦片（当前 {} 个）",
        safe_tiles,
        (cols as u64) * (rows as u64),
    ));

    // 建议 2: 如果不是流式 TIFF，建议切换
    if !matches!(format, FormatClass::StreamingTiff | FormatClass::TileByTile) {
        suggestions.push("请改用 GeoTIFF 格式（启用流式导出，内存消耗极低）".to_string());
    }

    // 建议 3: 如果有裁剪，建议关闭
    if has_crop && format != FormatClass::TileByTile {
        suggestions.push("请关闭多边形裁剪以降低内存消耗".to_string());
    }

    // 建议 4: 提升预算
    suggestions.push(format!(
        "请在设置中提升内存预算（当前 {} MB，需 {} MB）",
        budget_mb, peak_mb
    ));

    BudgetCheckResult {
        estimated_peak_bytes: estimated_peak,
        budget_bytes,
        allowed: false,
        suggestions,
    }
}

/// 将超预算结果转为用户友好的预警信息
pub fn format_budget_warning(result: &BudgetCheckResult) -> String {
    let peak_mb = result.estimated_peak_bytes / (1024 * 1024);
    let budget_mb = result.budget_bytes / (1024 * 1024);
    let mut msg = format!(
        "预估内存超过预算：预估峰值 {} MB，预算 {} MB。仅作提示，继续下载和导出。\n建议：\n",
        peak_mb, budget_mb
    );
    for (i, s) in result.suggestions.iter().enumerate() {
        msg.push_str(&format!("{}. {}\n", i + 1, s));
    }
    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_tile_directory_estimates_paths_instead_of_a_full_canvas() {
        let result = check_budget(620, 543, FormatClass::TileByTile, false, 2048);
        assert!(result.allowed);
        assert!(result.estimated_peak_bytes < 512 * 1024 * 1024);
        let cropped = check_budget(620, 543, FormatClass::TileByTile, true, 2048);
        assert_eq!(result.estimated_peak_bytes, cropped.estimated_peak_bytes);
        let small_budget = check_budget(620, 543, FormatClass::TileByTile, false, 256);
        assert!(!small_budget.allowed);
        assert!(!small_budget.suggestions.iter().any(|s| s.contains("GeoTIFF")));
        assert!(check_budget(620, 543, FormatClass::TileByTile, false, 0).allowed);
    }

    #[test]
    fn streaming_tiff_always_low_memory() {
        // 500x500 tiles streaming: 500*500*256*256*3*0.05 ≈ 2.3 GB, but budget 4096 MB
        let r = check_budget(500, 500, FormatClass::StreamingTiff, false, 4096);
        assert!(r.allowed);
        // Same area full TIFF would be ~46 GB → would fail
        let r2 = check_budget(500, 500, FormatClass::FullTiff, false, 4096);
        assert!(!r2.allowed);
    }

    #[test]
    fn small_area_passes() {
        // 100x100 tiles, full TIFF, no crop → ~100*100*256*256*3*2 = ~3.7 GB → 2GB budget should fail
        let r = check_budget(100, 100, FormatClass::FullTiff, false, 2048);
        assert!(!r.allowed);

        // 50x50 tiles → ~0.9 GB → passes
        let r = check_budget(50, 50, FormatClass::FullTiff, false, 2048);
        assert!(r.allowed);
    }

    #[test]
    fn crop_increases_peak() {
        // Same area, with crop needs more memory
        let no_crop = check_budget(50, 50, FormatClass::FullTiff, false, 2048);
        let with_crop = check_budget(50, 50, FormatClass::FullTiff, true, 2048);
        assert!(with_crop.estimated_peak_bytes > no_crop.estimated_peak_bytes);
    }

    #[test]
    fn suggestions_include_alternatives() {
        let r = check_budget(200, 200, FormatClass::RasterImage, true, 2048);
        assert!(!r.allowed);
        assert!(r.suggestions.len() >= 3);
        assert!(r.suggestions.iter().any(|s| s.contains("GeoTIFF")));
        assert!(r.suggestions.iter().any(|s| s.contains("多边形裁剪")));
    }

    #[test]
    fn custom_budget_is_not_clamped() {
        let r = check_budget(10, 10, FormatClass::FullTiff, false, 100);
        assert_eq!(r.budget_bytes, 100 * 1024 * 1024);
        let r = check_budget(200, 200, FormatClass::RasterImage, true, 98_304);
        assert_eq!(r.budget_bytes, 98_304 * 1024 * 1024);
        assert!(r.allowed);
    }

    #[test]
    fn unlimited_budget_disables_warnings() {
        let r = check_budget(200, 200, FormatClass::RasterImage, true, 0);
        assert!(r.allowed);
        assert_eq!(r.budget_bytes, 0);
        assert!(r.estimated_peak_bytes > 0);
        assert!(r.suggestions.is_empty());
    }

    #[test]
    fn large_budget_does_not_overflow() {
        let r = check_budget(200, 200, FormatClass::RasterImage, true, u64::MAX);
        assert!(r.allowed);
        assert_eq!(r.budget_bytes, u64::MAX);
    }

    #[test]
    fn format_warning_readable() {
        let r = check_budget(200, 200, FormatClass::RasterImage, true, 2048);
        let msg = format_budget_warning(&r);
        assert!(msg.contains("预估内存超过预算"));
        assert!(msg.contains("仅作提示，继续下载和导出"));
        assert!(msg.contains("建议"));
    }

    #[test]
    fn zero_tiles_passes() {
        let r = check_budget(0, 0, FormatClass::RasterImage, true, 2048);
        assert!(r.allowed);
    }

    #[test]
    fn single_tile_passes() {
        let r = check_budget(1, 1, FormatClass::RasterImage, true, 512);
        assert!(r.allowed);
    }
}
