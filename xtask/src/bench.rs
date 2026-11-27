//! Parse runtime logs into structured numeric benchmark summaries (`bench/YYYY-MM-DD-HHMM.json`).
//!
//! No game files, hashes, or addresses are ever written to the output report.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

/// Complete benchmark summary extracted from a single runtime log.
#[derive(Debug, Clone, PartialEq)]
pub struct BenchReport {
    pub schema_version: u32,
    pub label: Option<String>,
    pub stage: &'static str,
    pub frames: FrameMetrics,
    pub cpu_gpu: CpuGpuMetrics,
    pub memory: MemoryMetrics,
    pub shaders: ShaderMetrics,
    pub errors: ErrorMetrics,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FrameMetrics {
    pub count: usize,
    pub long_frames: usize,
    pub fps_avg: f64,
    pub fps_min: f64,
    pub fps_1pct_low: f64,
    pub frame_ms_p50: f64,
    pub frame_ms_p95: f64,
    pub frame_ms_p99: f64,
    pub frame_ms_max: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CpuGpuMetrics {
    pub preparing_ms_avg: f64,
    pub preparing_ms_p95: f64,
    pub waiting_for_card_ms_avg: f64,
    pub gpu_ms_avg: f64,
    pub gpu_ms_p95: f64,
    pub copy_back_ms_avg: f64,
    pub max_draws_per_frame: f64,
    pub bottleneck: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryMetrics {
    pub vram_peak_mb: u64,
    pub vram_budget_min_mb: u64,
    pub shared_vram_peak_mb: u64,
    pub commit_peak_mb: u64,
    pub process_private_peak_mb: u64,
    pub ram_free_min_mb: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ShaderMetrics {
    pub pipelines_refused: usize,
    pub compute_pipelines_refused: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ErrorMetrics {
    pub e_outofmemory: usize,
    pub device_removed: usize,
    pub tdr_or_hung: usize,
    pub error_lines: usize,
}

/// Reads `log_path`, parses its metrics, writes JSON to `out` (or
/// `<root>/bench/YYYY-MM-DD-HHMM.json`), and returns the output path.
pub fn run(root: &Path, log_path: &Path, out: Option<&Path>, label: Option<&str>) -> Result<PathBuf> {
    let text = fs::read_to_string(log_path)
        .with_context(|| format!("reading runtime log {}", log_path.display()))?;
    let report = parse_log(&text, label);

    let out_path = match out {
        Some(p) => p.to_path_buf(),
        None => {
            let stamp = extract_log_stamp(&text).unwrap_or_else(current_utc_stamp);
            root.join("bench").join(format!("{stamp}.json"))
        }
    };

    if let Some(parent) = out_path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating output directory {}", parent.display()))?;
    }

    let json = report.to_json();
    fs::write(&out_path, json)
        .with_context(|| format!("writing benchmark report {}", out_path.display()))?;

    println!(
        "bench: stage={} frames={} fps_avg={:.2} fps_1pct_low={:.2} p95={:.1}ms preparing_avg={:.1}ms gpu_avg={:.1}ms bottleneck={} vram_peak={}MB commit_peak={}MB refused={} errors(oom={},removed={},tdr={}) -> {}",
        report.stage,
        report.frames.count,
        report.frames.fps_avg,
        report.frames.fps_1pct_low,
        report.frames.frame_ms_p95,
        report.cpu_gpu.preparing_ms_avg,
        report.cpu_gpu.gpu_ms_avg,
        report.cpu_gpu.bottleneck,
        report.memory.vram_peak_mb,
        report.memory.commit_peak_mb,
        report.shaders.pipelines_refused,
        report.errors.e_outofmemory,
        report.errors.device_removed,
        report.errors.tdr_or_hung,
        out_path.display(),
    );

    Ok(out_path)
}

/// Parses a runtime log string into a [`BenchReport`] without touching disk.
pub fn parse_log(text: &str, label: Option<&str>) -> BenchReport {
    let mut per_frame_ms: Vec<f64> = Vec::new();
    let mut summary_fps: Vec<f64> = Vec::new();
    let mut summary_frame_ms: Vec<f64> = Vec::new();
    let mut preparing_ms: Vec<f64> = Vec::new();
    let mut waiting_for_card_ms: Vec<f64> = Vec::new();
    let mut gpu_ms: Vec<f64> = Vec::new();
    let mut copy_back_ms: Vec<f64> = Vec::new();

    let mut long_frames = 0usize;
    let mut max_draws_per_frame = 0.0_f64;

    let mut vram_peak_mb = 0_u64;
    let mut vram_budget_min_mb = u64::MAX;
    let mut shared_vram_peak_mb = 0_u64;
    let mut commit_peak_mb = 0_u64;
    let mut process_private_peak_mb = 0_u64;
    let mut ram_free_min_mb = u64::MAX;

    let mut pipelines_refused = 0usize;
    let mut compute_pipelines_refused = 0usize;

    let mut e_outofmemory = 0usize;
    let mut device_removed = 0usize;
    let mut tdr_or_hung = 0usize;
    let mut error_lines = 0usize;

    let mut saw_device_ready = false;
    let mut saw_runtime_activity = false;
    let mut saw_present = false;

    for line in text.lines() {
        let lower = line.to_ascii_lowercase();

        if lower.contains("victory_hle::") || lower.contains("victory_gpu::") {
            saw_runtime_activity = true;
        }
        if line.contains("host D3D12 device ready") {
            saw_device_ready = true;
        }
        if line.contains("Present") || line.contains("frame times:") || line.contains("ms/frame") {
            saw_present = true;
        }

        if line.contains(" ERROR ") {
            error_lines += 1;
        }
        if lower.contains("e_outofmemory") || lower.contains("0x8007000e") {
            e_outofmemory += 1;
        }
        if !lower.contains("device-removed data on:")
            && (lower.contains("device removed")
                || lower.contains("dxgi_error_device_removed")
                || lower.contains("0x887a0005"))
        {
            device_removed += 1;
        }
        if lower.contains("dxgi_error_device_hung")
            || lower.contains("dxgi_error_device_reset")
            || lower.contains("0x887a0006")
            || lower.contains("0x887a0007")
        {
            tdr_or_hung += 1;
        }

        if line.contains("host pipeline refused") {
            pipelines_refused += 1;
            if line.contains("shaders=[compute") {
                compute_pipelines_refused += 1;
            }
        }

        if let Some(sample) = parse_host_fps_line(line) {
            summary_fps.push(sample.fps);
            summary_frame_ms.push(sample.frame_ms);
            preparing_ms.push(sample.preparing_ms);
            gpu_ms.push(sample.gpu_ms);
            copy_back_ms.push(sample.copy_back_ms);
            max_draws_per_frame = max_draws_per_frame.max(sample.draws_per_frame);
        }

        if let Some((long_cnt, values)) = parse_framelog_times_line(line) {
            long_frames += long_cnt;
            per_frame_ms.extend(values);
        }

        if line.contains("usual frame") {
            if let Some(prep) = extract_number_after(line, "preparing ") {
                preparing_ms.push(prep);
            }
            if let Some(wait) = extract_number_after(line, "waiting for the card ") {
                waiting_for_card_ms.push(wait);
            }
            if let Some(draws) = extract_number_before(line, " draws") {
                max_draws_per_frame = max_draws_per_frame.max(draws);
            }
            if let Some(free) = extract_number_before(line, " MB free") {
                ram_free_min_mb = ram_free_min_mb.min(free as u64);
            }
        }

        if line.contains("video memory in the") {
            if let Some(vram) = extract_number_after(line, "the card's own in use at most ") {
                vram_peak_mb = vram_peak_mb.max(vram as u64);
            }
            if let Some(budget) = extract_number_after(line, "its budget at least ")
                && budget > 0.0
            {
                vram_budget_min_mb = vram_budget_min_mb.min(budget as u64);
            }
            if let Some(shared) = extract_number_after(line, "the PC's shared in use at most ") {
                shared_vram_peak_mb = shared_vram_peak_mb.max(shared as u64);
            }
        }

        if let Some(rest) = line.split("the PC by the minute: ").nth(1) {
            if let Some(free) = extract_number_before(rest, " MB free") {
                ram_free_min_mb = ram_free_min_mb.min(free as u64);
            }
            if let Some(committed) = extract_number_before(rest, " MB committed") {
                commit_peak_mb = commit_peak_mb.max(committed as u64);
            }
            if let Some(private) = extract_number_before(rest, " MB private") {
                process_private_peak_mb = process_private_peak_mb.max(private as u64);
            }
        }
    }

    let frame_samples = if !per_frame_ms.is_empty() {
        &per_frame_ms
    } else {
        &summary_frame_ms
    };

    let frames = compute_frame_metrics(frame_samples, &summary_fps, long_frames);

    let preparing_ms_avg = mean(&preparing_ms);
    let preparing_ms_p95 = percentile(&preparing_ms, 0.95);
    let waiting_for_card_ms_avg = mean(&waiting_for_card_ms);
    let gpu_ms_avg = mean(&gpu_ms);
    let gpu_ms_p95 = percentile(&gpu_ms, 0.95);
    let copy_back_ms_avg = mean(&copy_back_ms);

    let bottleneck = classify_bottleneck(
        !preparing_ms.is_empty(),
        preparing_ms_avg,
        !gpu_ms.is_empty(),
        gpu_ms_avg,
    );

    let stage = classify_stage(
        max_draws_per_frame,
        frames.count,
        saw_present,
        saw_device_ready,
        saw_runtime_activity,
    );

    BenchReport {
        schema_version: 1,
        label: label.map(ToOwned::to_owned),
        stage,
        frames,
        cpu_gpu: CpuGpuMetrics {
            preparing_ms_avg,
            preparing_ms_p95,
            waiting_for_card_ms_avg,
            gpu_ms_avg,
            gpu_ms_p95,
            copy_back_ms_avg,
            max_draws_per_frame,
            bottleneck,
        },
        memory: MemoryMetrics {
            vram_peak_mb,
            vram_budget_min_mb: if vram_budget_min_mb == u64::MAX {
                0
            } else {
                vram_budget_min_mb
            },
            shared_vram_peak_mb,
            commit_peak_mb,
            process_private_peak_mb,
            ram_free_min_mb: if ram_free_min_mb == u64::MAX {
                0
            } else {
                ram_free_min_mb
            },
        },
        shaders: ShaderMetrics {
            pipelines_refused,
            compute_pipelines_refused,
        },
        errors: ErrorMetrics {
            e_outofmemory,
            device_removed,
            tdr_or_hung,
            error_lines,
        },
    }
}

struct HostFpsSample {
    fps: f64,
    frame_ms: f64,
    preparing_ms: f64,
    gpu_ms: f64,
    copy_back_ms: f64,
    draws_per_frame: f64,
}

fn parse_host_fps_line(line: &str) -> Option<HostFpsSample> {
    if !line.contains(" ms/frame (") {
        return None;
    }
    let fps = extract_number_before(line, " fps,")?;
    let frame_ms = extract_number_before(line, " ms/frame (")?;
    let preparing_ms = extract_number_before(line, " preparing:")?;
    let gpu_ms = extract_number_before(line, " on the GPU;").unwrap_or(0.0);
    let copy_back_ms = extract_number_before(line, " copying back ").unwrap_or(0.0);
    let draws_per_frame = extract_number_before(line, " draws/frame").unwrap_or(0.0);
    Some(HostFpsSample {
        fps,
        frame_ms,
        preparing_ms,
        gpu_ms,
        copy_back_ms,
        draws_per_frame,
    })
}

fn parse_framelog_times_line(line: &str) -> Option<(usize, Vec<f64>)> {
    let (_, after_each) = line.split_once("; each: ")?;
    let long_cnt = extract_number_before(line, " long (")
        .map(|v| v.max(0.0) as usize)
        .unwrap_or(0);
    let values = after_each
        .split_whitespace()
        .filter_map(|tok| tok.parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v > 0.0)
        .collect();
    Some((long_cnt, values))
}

fn extract_number_after(line: &str, marker: &str) -> Option<f64> {
    let rest = line.split_once(marker)?.1;
    let tok = rest.split_whitespace().next()?;
    tok.trim_end_matches([',', ';', ')', ':'])
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

fn extract_number_before(line: &str, marker: &str) -> Option<f64> {
    let prefix = line.split_once(marker)?.0;
    let tok = prefix.split_whitespace().next_back()?;
    tok.trim_start_matches(['(', '['])
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}

fn compute_frame_metrics(
    frame_samples: &[f64],
    summary_fps: &[f64],
    long_frames: usize,
) -> FrameMetrics {
    if frame_samples.is_empty() {
        let fps_avg = mean(summary_fps);
        let fps_min = summary_fps.iter().copied().fold(f64::INFINITY, f64::min);
        return FrameMetrics {
            count: summary_fps.len(),
            long_frames,
            fps_avg,
            fps_min: if fps_min.is_finite() { fps_min } else { 0.0 },
            fps_1pct_low: 0.0,
            frame_ms_p50: 0.0,
            frame_ms_p95: 0.0,
            frame_ms_p99: 0.0,
            frame_ms_max: 0.0,
        };
    }

    let mut sorted = frame_samples.to_vec();
    sorted.sort_by(f64::total_cmp);

    let mean_ms = mean(&sorted);
    let fps_avg = if mean_ms > 0.0 {
        1000.0 / mean_ms
    } else {
        mean(summary_fps)
    };
    let max_ms = *sorted.last().unwrap_or(&0.0);
    let fps_min = if max_ms > 0.0 { 1000.0 / max_ms } else { 0.0 };

    // 1% low = 1000 / mean of the slowest 1% frames (at least 1 frame).
    let worst_count = (sorted.len() / 100).max(1);
    let worst_mean_ms = mean(&sorted[sorted.len() - worst_count..]);
    let fps_1pct_low = if worst_mean_ms > 0.0 {
        1000.0 / worst_mean_ms
    } else {
        0.0
    };

    FrameMetrics {
        count: sorted.len(),
        long_frames,
        fps_avg,
        fps_min,
        fps_1pct_low,
        frame_ms_p50: percentile_sorted(&sorted, 0.50),
        frame_ms_p95: percentile_sorted(&sorted, 0.95),
        frame_ms_p99: percentile_sorted(&sorted, 0.99),
        frame_ms_max: max_ms,
    }
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

fn percentile(values: &[f64], q: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    percentile_sorted(&sorted, q)
}

fn percentile_sorted(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    // Nearest-rank percentile: ceil(q * N) - 1 (clamped to valid range).
    let rank = (q.clamp(0.0, 1.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn classify_bottleneck(
    has_cpu: bool,
    preparing_ms_avg: f64,
    has_gpu: bool,
    gpu_ms_avg: f64,
) -> &'static str {
    if !has_cpu && !has_gpu {
        return "unknown";
    }
    if has_gpu && (!has_cpu || gpu_ms_avg > preparing_ms_avg * 1.15) {
        "gpu"
    } else if has_cpu && (!has_gpu || preparing_ms_avg > gpu_ms_avg * 1.15) {
        "cpu"
    } else {
        "balanced"
    }
}

fn classify_stage(
    max_draws: f64,
    frame_count: usize,
    saw_present: bool,
    saw_device_ready: bool,
    saw_runtime_activity: bool,
) -> &'static str {
    if max_draws >= 500.0 {
        "world"
    } else if frame_count >= 10 {
        "menu"
    } else if frame_count >= 1 || saw_present {
        "first_frame"
    } else if saw_device_ready {
        "device_ready"
    } else if saw_runtime_activity {
        "exe_started"
    } else {
        "unknown"
    }
}

/// Extracts `YYYY-MM-DD-HHMM` from the first ISO-8601 timestamp (`YYYY-MM-DDTHH:MM:...`) in the log.
fn extract_log_stamp(text: &str) -> Option<String> {
    for line in text.lines().take(50) {
        let b = line.as_bytes();
        if b.len() >= 16
            && b[4] == b'-'
            && b[7] == b'-'
            && b[10] == b'T'
            && b[13] == b':'
            && b[0..4].iter().all(u8::is_ascii_digit)
            && b[5..7].iter().all(u8::is_ascii_digit)
            && b[8..10].iter().all(u8::is_ascii_digit)
            && b[11..13].iter().all(u8::is_ascii_digit)
            && b[14..16].iter().all(u8::is_ascii_digit)
        {
            return Some(format!(
                "{}-{}-{}-{}{}",
                &line[0..4],
                &line[5..7],
                &line[8..10],
                &line[11..13],
                &line[14..16]
            ));
        }
    }
    None
}

fn current_utc_stamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let hour = rem / 3_600;
    let minute = (rem % 3_600) / 60;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}-{hour:02}{minute:02}")
}

/// Converts days since 1970-01-01 to `(year, month, day)` in UTC.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year, m as u32, d as u32)
}

impl BenchReport {
    pub fn to_json(&self) -> String {
        let mut out = String::with_capacity(1024);
        let label_json = match &self.label {
            Some(l) => format!("\"{}\"", escape_json(l)),
            None => "null".to_owned(),
        };
        let _ = writeln!(out, "{{");
        let _ = writeln!(out, "  \"schema_version\": {},", self.schema_version);
        let _ = writeln!(out, "  \"label\": {label_json},");
        let _ = writeln!(out, "  \"stage\": \"{}\",", self.stage);
        let _ = writeln!(out, "  \"frames\": {{");
        let _ = writeln!(out, "    \"count\": {},", self.frames.count);
        let _ = writeln!(out, "    \"long_frames\": {},", self.frames.long_frames);
        let _ = writeln!(out, "    \"fps_avg\": {:.2},", self.frames.fps_avg);
        let _ = writeln!(out, "    \"fps_min\": {:.2},", self.frames.fps_min);
        let _ = writeln!(out, "    \"fps_1pct_low\": {:.2},", self.frames.fps_1pct_low);
        let _ = writeln!(out, "    \"frame_ms_p50\": {:.2},", self.frames.frame_ms_p50);
        let _ = writeln!(out, "    \"frame_ms_p95\": {:.2},", self.frames.frame_ms_p95);
        let _ = writeln!(out, "    \"frame_ms_p99\": {:.2},", self.frames.frame_ms_p99);
        let _ = writeln!(out, "    \"frame_ms_max\": {:.2}", self.frames.frame_ms_max);
        let _ = writeln!(out, "  }},");
        let _ = writeln!(out, "  \"cpu_gpu\": {{");
        let _ = writeln!(out, "    \"preparing_ms_avg\": {:.2},", self.cpu_gpu.preparing_ms_avg);
        let _ = writeln!(out, "    \"preparing_ms_p95\": {:.2},", self.cpu_gpu.preparing_ms_p95);
        let _ = writeln!(
            out,
            "    \"waiting_for_card_ms_avg\": {:.2},",
            self.cpu_gpu.waiting_for_card_ms_avg
        );
        let _ = writeln!(out, "    \"gpu_ms_avg\": {:.2},", self.cpu_gpu.gpu_ms_avg);
        let _ = writeln!(out, "    \"gpu_ms_p95\": {:.2},", self.cpu_gpu.gpu_ms_p95);
        let _ = writeln!(out, "    \"copy_back_ms_avg\": {:.2},", self.cpu_gpu.copy_back_ms_avg);
        let _ = writeln!(
            out,
            "    \"max_draws_per_frame\": {:.0},",
            self.cpu_gpu.max_draws_per_frame
        );
        let _ = writeln!(out, "    \"bottleneck\": \"{}\"", self.cpu_gpu.bottleneck);
        let _ = writeln!(out, "  }},");
        let _ = writeln!(out, "  \"memory\": {{");
        let _ = writeln!(out, "    \"vram_peak_mb\": {},", self.memory.vram_peak_mb);
        let _ = writeln!(out, "    \"vram_budget_min_mb\": {},", self.memory.vram_budget_min_mb);
        let _ = writeln!(out, "    \"shared_vram_peak_mb\": {},", self.memory.shared_vram_peak_mb);
        let _ = writeln!(out, "    \"commit_peak_mb\": {},", self.memory.commit_peak_mb);
        let _ = writeln!(
            out,
            "    \"process_private_peak_mb\": {},",
            self.memory.process_private_peak_mb
        );
        let _ = writeln!(out, "    \"ram_free_min_mb\": {}", self.memory.ram_free_min_mb);
        let _ = writeln!(out, "  }},");
        let _ = writeln!(out, "  \"shaders\": {{");
        let _ = writeln!(out, "    \"pipelines_refused\": {},", self.shaders.pipelines_refused);
        let _ = writeln!(
            out,
            "    \"compute_pipelines_refused\": {}",
            self.shaders.compute_pipelines_refused
        );
        let _ = writeln!(out, "  }},");
        let _ = writeln!(out, "  \"errors\": {{");
        let _ = writeln!(out, "    \"e_outofmemory\": {},", self.errors.e_outofmemory);
        let _ = writeln!(out, "    \"device_removed\": {},", self.errors.device_removed);
        let _ = writeln!(out, "    \"tdr_or_hung\": {},", self.errors.tdr_or_hung);
        let _ = writeln!(out, "    \"error_lines\": {}", self.errors.error_lines);
        let _ = writeln!(out, "  }}");
        let _ = writeln!(out, "}}");
        out
    }
}

fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_synthetic_open_world_log() {
        let log = "\
2026-11-26T21:10:00.100Z  INFO victory_gpu::host: device-removed data on: faults: page faults only, no breadcrumbs
2026-11-26T21:10:01.000Z  INFO victory_gpu::host: host D3D12 device ready
2026-11-26T21:10:02.000Z  WARN victory_gpu::pipeline::host: host pipeline refused error=E_INVALIDARG shaders=[compute 1111111111111111]
2026-11-26T21:10:02.100Z  WARN victory_gpu::pipeline::host: host pipeline refused error=E_INVALIDARG shaders=[vertex 2222222222222222]
2026-11-26T21:10:05.000Z  INFO victory_gpu::framelog: frame times: 10 frames in 1.0 s: shortest 30 ms, median 33, longest 100; 1 long (over 73 ms, the usual frame of the last 64 being 33); each: 30 31 32 33 33 34 35 40 50 100
2026-11-26T21:10:05.001Z  INFO victory_gpu::framelog: usual frame (the mean of 9 not long): 35 ms | render thread: ran 2 ms (user 1, kernel 1); in the present call 1.0 (lines and settling 0.0, copy and show 1.0), in submit calls 0.2, in list calls 0.4, paced 0.0 | submit thread: at work 40.0 ms (user 30.0, kernel 10.0); 12 submissions, preparing 18.0, waiting for the card 6.0; copies 1.0 ms 2.0 MB; copy-back 0.5 ms 0.10 MB | card: idle 0.0 ms (0.0 behind us); by its own clock, of the lists of a moment ago: our send-and-hash lists 1.0 ms in 1, our compare lists 1.0 ms in 1, all other lists 32.0 ms in 10 | 2400 draws | 4200 MB free | found on a slow core: 0 of 1
2026-11-26T21:10:06.000Z  INFO victory_gpu::host: video memory in the 5000 ms since the last such line, at 40 looks: the card's own in use at most 3420 MB, its budget at least 3550 MB, the PC's shared in use at most 512 MB; made in that time: upload buffers 16 MB
2026-11-26T21:10:06.100Z  INFO victory_gpu::host: the PC by the minute: 3800 MB free, 14500 MB committed, the kernel's own 200 MB paged and 150 MB not paged, 400 handles, the process 9200 MB private; 100 pipelines first handed to a draw so far
2026-11-26T21:10:06.200Z  INFO victory_gpu::host: 22.5 fps, 44.4 ms/frame (20.0 preparing: copies 2.0 ms 1.0 MB, textures 10.0 ms 12.0 MB; 36.0 on the GPU; 1.5 copying back 0.4 MB), 3100 draws/frame, dropped 0 without a host pipeline (0 being made) and 0 indirect
";

        let rep = parse_log(log, Some("synthetic-world"));
        assert_eq!(rep.stage, "world");
        assert_eq!(rep.frames.count, 10);
        assert_eq!(rep.frames.long_frames, 1);
        assert!((rep.frames.frame_ms_p50 - 33.0).abs() < 0.01);
        assert!((rep.frames.frame_ms_p95 - 100.0).abs() < 0.01);
        assert!((rep.frames.fps_1pct_low - 10.0).abs() < 0.01);
        assert!((rep.cpu_gpu.preparing_ms_avg - 19.0).abs() < 0.01);
        assert!((rep.cpu_gpu.gpu_ms_avg - 36.0).abs() < 0.01);
        assert!((rep.cpu_gpu.waiting_for_card_ms_avg - 6.0).abs() < 0.01);
        assert!((rep.cpu_gpu.max_draws_per_frame - 3100.0).abs() < 0.01);
        assert_eq!(rep.cpu_gpu.bottleneck, "gpu");
        assert_eq!(rep.memory.vram_peak_mb, 3420);
        assert_eq!(rep.memory.vram_budget_min_mb, 3550);
        assert_eq!(rep.memory.shared_vram_peak_mb, 512);
        assert_eq!(rep.memory.commit_peak_mb, 14500);
        assert_eq!(rep.memory.process_private_peak_mb, 9200);
        assert_eq!(rep.memory.ram_free_min_mb, 3800);
        assert_eq!(rep.shaders.pipelines_refused, 2);
        assert_eq!(rep.shaders.compute_pipelines_refused, 1);
        assert_eq!(rep.errors.device_removed, 0);
        assert_eq!(rep.errors.e_outofmemory, 0);
        assert_eq!(extract_log_stamp(log).as_deref(), Some("2026-11-26-2110"));
    }

    #[test]
    fn detects_oom_and_device_removed_in_menu_log() {
        let log = "\
2026-11-26T21:15:00.000Z  INFO victory_gpu::host: host D3D12 device ready
2026-11-26T21:15:05.000Z  INFO victory_gpu::framelog: frame times: 12 frames in 0.4 s: shortest 30 ms, median 33, longest 40; 0 long (over 73 ms, the usual frame of the last 64 being 33); each: 30 31 32 33 33 33 33 33 34 35 36 40
2026-11-26T21:15:06.000Z ERROR victory_gpu::texture: allocation failed E_OUTOFMEMORY
2026-11-26T21:15:07.000Z ERROR victory_gpu::host: Present failed: DXGI_ERROR_DEVICE_REMOVED (0x887A0005)
";
        let rep = parse_log(log, None);
        assert_eq!(rep.stage, "menu");
        assert_eq!(rep.errors.e_outofmemory, 1);
        assert_eq!(rep.errors.device_removed, 1);
        assert_eq!(rep.errors.error_lines, 2);
    }

    #[test]
    fn writes_valid_json_file_in_temp_dir() {
        let dir = std::env::temp_dir().join(format!("lucia_bench_test_{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let log_file = dir.join("sample.log");
        let out_file = dir.join("out.json");
        fs::write(
            &log_file,
            "2026-11-26T21:20:00.000Z  INFO victory_gpu::host: host D3D12 device ready\n",
        )
        .unwrap();

        let written = run(&dir, &log_file, Some(&out_file), Some("smoke\"test")).unwrap();
        assert_eq!(written, out_file);
        let json = fs::read_to_string(&out_file).unwrap();
        assert!(json.contains("\"schema_version\": 1"));
        assert!(json.contains("\"label\": \"smoke\\\"test\""));
        assert!(json.contains("\"stage\": \"device_ready\""));

        let _ = fs::remove_dir_all(&dir);
    }
}
