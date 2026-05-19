//! OCR text recognition tools — thin orchestrator.
//!
//! Backends:
//! - `ocr_ocrs`     — ocrs (Rust-native) and PaddleOCR-VL engines
//! - `ocr_tesseract` — Tesseract CLI (cross-platform)
//! - `ocr_winrt`    — Windows.Media.Ocr WinRT API (Windows only)
//! - `ocr_macos`    — macOS Vision framework via embedded Swift (macOS only)
//! - `ocr_common`   — shared types, cache, capture helpers, platform dispatchers

use serde_json::Value;
use crate::NativeToolResult;

mod ocr_common;
mod ocr_ocrs;
mod ocr_tesseract;
#[cfg(windows)]
mod ocr_winrt;
#[cfg(target_os = "macos")]
mod ocr_macos;

pub(super) use ocr_common::{
    OcrCaptureTarget, OcrCachePayload,
    ocr_cache_settings, get_cached_ocr_payload, update_cached_ocr_payload,
    capture_ocr_target, upscale_for_ocr,
    ocr_find_text, ocr_png_and_search,
};
pub(super) use ocr_ocrs::{ocr_image_ocrs, ocr_find_text_ocrs, ocr_image_vlm};
pub(super) use ocr_tesseract::{ocr_image_tesseract, ocr_find_text_tesseract};
#[cfg(windows)]
pub(super) use ocr_winrt::{ocr_image_winrt, ocr_find_text_winrt};
#[cfg(target_os = "macos")]
pub(super) use ocr_macos::{ocr_image_vision, ocr_find_text_vision};

// ─── Windows: tool_ocr_screen ─────────────────────────────────────────────────

#[cfg(windows)]
pub fn tool_ocr_screen(args: &Value) -> NativeToolResult {
    // confidence_min is accepted for API consistency; WinRT text-only mode has no per-line confidence.
    let _confidence_min: f64 = args.get("confidence_min").and_then(crate::parse_float).unwrap_or(0.0);
    let language = args.get("language").and_then(|v| v.as_str());

    let target = match capture_ocr_target(args, "ocr_screen") {
        Ok(t) => t,
        Err(r) => return r,
    };
    let (cache_max_age_ms, cache_threshold_pct) = ocr_cache_settings(args);
    let cache_key = format!("ocr_screen:{}:{:?}", target.region_desc, language);
    if let Some(OcrCachePayload::Text(text)) =
        get_cached_ocr_payload(&cache_key, &target.raw, cache_max_age_ms, cache_threshold_pct)
    {
        let line_count = text.lines().count();
        return NativeToolResult::text_only(format!(
            "OCR{} (cached): {line_count} lines\n{text}",
            target.region_desc
        ));
    }
    let OcrCaptureTarget { image: raw_image, raw, region_desc, .. } = target;
    let image = upscale_for_ocr(&raw_image);

    let engine_pref = args.get("engine").and_then(|v| v.as_str()).unwrap_or("auto");
    let result = match engine_pref {
        "vlm" | "paddleocr" => ocr_image_vlm(&raw_image),
        "ocrs" => ocr_image_ocrs(&image),
        "tesseract" => ocr_image_tesseract(&image, language),
        "winrt" | "native" => crate::spawn_with_timeout(crate::DEFAULT_THREAD_TIMEOUT, {
            let img = image.clone();
            move || ocr_image_winrt(&img)
        }).and_then(|r| r),
        _ => {
            // Auto: tesseract → ocrs → WinRT
            ocr_image_tesseract(&image, language)
                .or_else(|_| { eprintln!("[OCR] Tesseract unavailable, trying ocrs"); ocr_image_ocrs(&image) })
                .or_else(|_| {
                    eprintln!("[OCR] ocrs unavailable, falling back to WinRT");
                    crate::spawn_with_timeout(crate::DEFAULT_THREAD_TIMEOUT, {
                        let img = image.clone();
                        move || ocr_image_winrt(&img)
                    }).and_then(|r| r)
                })
        }
    };

    match result {
        Ok(text) => {
            update_cached_ocr_payload(cache_key, raw, OcrCachePayload::Text(text.clone()));
            if text.is_empty() {
                NativeToolResult::text_only(format!("OCR{}: no text detected", region_desc))
            } else {
                let line_count = text.lines().count();
                NativeToolResult::text_only(format!("OCR{}: {line_count} lines\n{text}", region_desc))
            }
        }
        Err(e) => crate::tool_error("ocr_screen", format!("OCR: {e}")),
    }
}

// ─── macOS: tool_ocr_screen ───────────────────────────────────────────────────

#[cfg(target_os = "macos")]
pub fn tool_ocr_screen(args: &Value) -> NativeToolResult {
    let language = args.get("language").and_then(|v| v.as_str());
    let _confidence_min: f64 = args.get("confidence_min").and_then(crate::parse_float).unwrap_or(0.0);
    let target = match capture_ocr_target(args, "ocr_screen") {
        Ok(t) => t,
        Err(r) => return r,
    };
    let (cache_max_age_ms, cache_threshold_pct) = ocr_cache_settings(args);
    let cache_key = format!("ocr_screen:{}:{:?}", target.region_desc, language);
    if let Some(OcrCachePayload::Text(text)) =
        get_cached_ocr_payload(&cache_key, &target.raw, cache_max_age_ms, cache_threshold_pct)
    {
        return NativeToolResult::text_only(format!("OCR{} (cached):\n{text}", target.region_desc));
    }
    let image = upscale_for_ocr(&target.image);
    let engine_pref = args.get("engine").and_then(|v| v.as_str()).unwrap_or("auto");
    let result = match engine_pref {
        "ocrs" => ocr_image_ocrs(&image),
        "tesseract" => ocr_image_tesseract(&image, language),
        "native" | "vision" => ocr_image_vision(&image, language),
        _ => ocr_image_vision(&image, language)
            .or_else(|_| ocr_image_tesseract(&image, language))
            .or_else(|_| ocr_image_ocrs(&image)),
    };
    match result {
        Ok(text) => {
            update_cached_ocr_payload(cache_key, target.raw, OcrCachePayload::Text(text.clone()));
            NativeToolResult::text_only(format!("OCR{}:\n{text}", target.region_desc))
        }
        Err(e) => crate::tool_error("ocr_screen", e),
    }
}

// ─── Linux: tool_ocr_screen ───────────────────────────────────────────────────

#[cfg(target_os = "linux")]
pub fn tool_ocr_screen(args: &Value) -> NativeToolResult {
    let language = args.get("language").and_then(|v| v.as_str());
    let _confidence_min: f64 = args.get("confidence_min").and_then(crate::parse_float).unwrap_or(0.0);
    let target = match capture_ocr_target(args, "ocr_screen") {
        Ok(t) => t,
        Err(r) => return r,
    };
    let (cache_max_age_ms, cache_threshold_pct) = ocr_cache_settings(args);
    let cache_key = format!("ocr_screen:{}:{:?}", target.region_desc, language);
    if let Some(OcrCachePayload::Text(text)) =
        get_cached_ocr_payload(&cache_key, &target.raw, cache_max_age_ms, cache_threshold_pct)
    {
        return NativeToolResult::text_only(format!("OCR{} (cached):\n{text}", target.region_desc));
    }
    let image = upscale_for_ocr(&target.image);
    let engine_pref = args.get("engine").and_then(|v| v.as_str()).unwrap_or("auto");
    let result = match engine_pref {
        "ocrs" => ocr_image_ocrs(&image),
        "tesseract" => ocr_image_tesseract(&image, language),
        _ => ocr_image_tesseract(&image, language).or_else(|_| ocr_image_ocrs(&image)),
    };
    match result {
        Ok(text) => {
            update_cached_ocr_payload(cache_key, target.raw, OcrCachePayload::Text(text.clone()));
            NativeToolResult::text_only(format!("OCR{}:\n{text}", target.region_desc))
        }
        Err(e) => crate::tool_error("ocr_screen", e),
    }
}

// ─── Windows: tool_ocr_find_text ──────────────────────────────────────────────

#[cfg(windows)]
pub fn tool_ocr_find_text(args: &Value) -> NativeToolResult {
    let search_text = match args.get("text").and_then(|v| v.as_str()) {
        Some(t) => t,
        None => return crate::tool_error("ocr_find_text", "'text' argument is required"),
    };
    let search = search_text.to_lowercase();
    let confidence_min = args.get("confidence_min").and_then(crate::parse_float).unwrap_or(0.0);
    let target = match capture_ocr_target(args, "ocr_find_text") {
        Ok(t) => t,
        Err(r) => return r,
    };
    let (cache_max_age_ms, cache_threshold_pct) = ocr_cache_settings(args);
    let cache_key = format!("ocr_find_text:{}:{}", target.region_desc, search);
    if let Some(OcrCachePayload::Matches(matches)) =
        get_cached_ocr_payload(&cache_key, &target.raw, cache_max_age_ms, cache_threshold_pct)
    {
        let filtered: Vec<_> = matches.into_iter().filter(|m| m.confidence >= confidence_min).collect();
        if filtered.is_empty() {
            return NativeToolResult::text_only(format!(
                "Text '{}' not found in{} (cached)", search_text, target.region_desc
            ));
        }
        let lines: Vec<String> = filtered.iter().map(|m| {
            format!("\"{}\" at ({:.0},{:.0}) {:.0}x{:.0} conf={:.2}", m.text, m.x, m.y, m.width, m.height, m.confidence)
        }).collect();
        return NativeToolResult::text_only(format!(
            "Found {} match(es) in{} (cached):\n{}", filtered.len(), target.region_desc, lines.join("\n")
        ));
    }
    let OcrCaptureTarget { image, raw, region_desc, offset_x, offset_y } = target;

    let engine_pref = args.get("engine").and_then(|v| v.as_str()).unwrap_or("auto");
    let result = match engine_pref {
        "ocrs" => ocr_find_text_ocrs(&image, &search, offset_x, offset_y),
        "tesseract" => ocr_find_text_tesseract(&image, &search, offset_x, offset_y),
        "winrt" | "native" => crate::spawn_with_timeout(crate::DEFAULT_THREAD_TIMEOUT, {
            let img = image.clone();
            let s = search.clone();
            move || ocr_find_text_winrt(&img, &s, offset_x, offset_y)
        }).and_then(|r| r),
        _ => {
            ocr_find_text_tesseract(&image, &search, offset_x, offset_y)
                .or_else(|_| { eprintln!("[OCR] Tesseract unavailable for find_text, trying ocrs"); ocr_find_text_ocrs(&image, &search, offset_x, offset_y) })
                .or_else(|_| {
                    eprintln!("[OCR] ocrs unavailable for find_text, falling back to WinRT");
                    crate::spawn_with_timeout(crate::DEFAULT_THREAD_TIMEOUT, {
                        let img = image.clone();
                        let s = search.clone();
                        move || ocr_find_text_winrt(&img, &s, offset_x, offset_y)
                    }).and_then(|r| r)
                })
        }
    };

    match result {
        Ok(matches) => {
            update_cached_ocr_payload(cache_key, raw, OcrCachePayload::Matches(matches.clone()));
            let filtered: Vec<_> = matches.into_iter().filter(|m| m.confidence >= confidence_min).collect();
            if filtered.is_empty() {
                NativeToolResult::text_only(format!("Text '{}' not found in{}", search_text, region_desc))
            } else {
                let lines: Vec<String> = filtered.iter().map(|m| {
                    format!("\"{}\" at ({:.0},{:.0}) {:.0}x{:.0} conf={:.2}", m.text, m.x, m.y, m.width, m.height, m.confidence)
                }).collect();
                NativeToolResult::text_only(format!("Found {} match(es) in{}:\n{}", filtered.len(), region_desc, lines.join("\n")))
            }
        }
        Err(e) => crate::tool_error("ocr_find_text", format!("OCR: {e}")),
    }
}

// ─── macOS: tool_ocr_find_text ────────────────────────────────────────────────

#[cfg(target_os = "macos")]
pub fn tool_ocr_find_text(args: &Value) -> NativeToolResult {
    let search = match args.get("text").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return crate::tool_error("ocr_find_text", "'text' is required"),
    };
    let confidence_min = args.get("confidence_min").and_then(crate::parse_float).unwrap_or(0.0);
    let target = match capture_ocr_target(args, "ocr_find_text") {
        Ok(t) => t,
        Err(r) => return r,
    };
    let (cache_max_age_ms, cache_threshold_pct) = ocr_cache_settings(args);
    let cache_key = format!("ocr_find_text:{}:{}", target.region_desc, search.to_lowercase());
    if let Some(OcrCachePayload::Matches(matches)) =
        get_cached_ocr_payload(&cache_key, &target.raw, cache_max_age_ms, cache_threshold_pct)
    {
        let filtered: Vec<_> = matches.into_iter().filter(|m| m.confidence >= confidence_min).collect();
        if filtered.is_empty() {
            return NativeToolResult::text_only(format!("Text '{search}' not found in{} (cached)", target.region_desc));
        }
        let mut lines = vec![format!("Found {} match(es) for '{search}' in{} (cached):", filtered.len(), target.region_desc)];
        for m in &filtered {
            lines.push(format!("  \"{}\" at ({:.0},{:.0}) {:.0}x{:.0} conf={:.2}", m.text, m.x, m.y, m.width, m.height, m.confidence));
        }
        return NativeToolResult::text_only(lines.join("\n"));
    }
    let result = match ocr_find_text_vision(&target.image, search, target.offset_x, target.offset_y, None) {
        Ok(m) => Ok(m),
        Err(_) => ocr_find_text_tesseract(&target.image, search, target.offset_x, target.offset_y),
    };
    match result {
        Ok(matches) if matches.is_empty() => {
            update_cached_ocr_payload(cache_key, target.raw, OcrCachePayload::Matches(Vec::new()));
            NativeToolResult::text_only(format!("Text '{search}' not found in{}", target.region_desc))
        }
        Ok(matches) => {
            update_cached_ocr_payload(cache_key, target.raw, OcrCachePayload::Matches(matches.clone()));
            let filtered: Vec<_> = matches.into_iter().filter(|m| m.confidence >= confidence_min).collect();
            let mut lines = vec![format!("Found {} match(es) for '{search}' in{}:", filtered.len(), target.region_desc)];
            for m in &filtered {
                lines.push(format!("  \"{}\" at ({:.0},{:.0}) {:.0}x{:.0} conf={:.2}", m.text, m.x, m.y, m.width, m.height, m.confidence));
            }
            NativeToolResult::text_only(lines.join("\n"))
        }
        Err(e) => crate::tool_error("ocr_find_text", e),
    }
}

// ─── Linux: tool_ocr_find_text ────────────────────────────────────────────────

#[cfg(target_os = "linux")]
pub fn tool_ocr_find_text(args: &Value) -> NativeToolResult {
    let search = match args.get("text").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return crate::tool_error("ocr_find_text", "'text' is required"),
    };
    let confidence_min = args.get("confidence_min").and_then(crate::parse_float).unwrap_or(0.0);
    let target = match capture_ocr_target(args, "ocr_find_text") {
        Ok(t) => t,
        Err(r) => return r,
    };
    let (cache_max_age_ms, cache_threshold_pct) = ocr_cache_settings(args);
    let cache_key = format!("ocr_find_text:{}:{}", target.region_desc, search.to_lowercase());
    if let Some(OcrCachePayload::Matches(matches)) =
        get_cached_ocr_payload(&cache_key, &target.raw, cache_max_age_ms, cache_threshold_pct)
    {
        let filtered: Vec<_> = matches.into_iter().filter(|m| m.confidence >= confidence_min).collect();
        if filtered.is_empty() {
            return NativeToolResult::text_only(format!("Text '{search}' not found in{} (cached)", target.region_desc));
        }
        let mut lines = vec![format!("Found {} match(es) for '{search}' in{} (cached):", filtered.len(), target.region_desc)];
        for m in &filtered {
            lines.push(format!("  \"{}\" at ({:.0},{:.0}) {:.0}x{:.0} conf={:.2}", m.text, m.x, m.y, m.width, m.height, m.confidence));
        }
        return NativeToolResult::text_only(lines.join("\n"));
    }
    match ocr_find_text_tesseract(&target.image, search, target.offset_x, target.offset_y) {
        Ok(matches) if matches.is_empty() => {
            update_cached_ocr_payload(cache_key, target.raw, OcrCachePayload::Matches(Vec::new()));
            NativeToolResult::text_only(format!("Text '{search}' not found in{}", target.region_desc))
        }
        Ok(matches) => {
            update_cached_ocr_payload(cache_key, target.raw, OcrCachePayload::Matches(matches.clone()));
            let filtered: Vec<_> = matches.into_iter().filter(|m| m.confidence >= confidence_min).collect();
            let mut lines = vec![format!("Found {} match(es) for '{search}' in{}:", filtered.len(), target.region_desc)];
            for m in &filtered {
                lines.push(format!("  \"{}\" at ({:.0},{:.0}) {:.0}x{:.0} conf={:.2}", m.text, m.x, m.y, m.width, m.height, m.confidence));
            }
            NativeToolResult::text_only(lines.join("\n"))
        }
        Err(e) => crate::tool_error("ocr_find_text", e),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::ocr_common::{clamp_region_to_monitor, get_cached_ocr_payload, update_cached_ocr_payload, OcrCachePayload, OcrMatch};

    #[test]
    fn test_clamp_region_to_monitor_inside() {
        let (x, y, w, h, ox, oy) =
            clamp_region_to_monitor(0, 0, 1920, 1080, 100, 200, 300, 400).unwrap();
        assert_eq!((x, y, w, h), (100, 200, 300, 400));
        assert_eq!((ox, oy), (100.0, 200.0));
    }

    #[test]
    fn test_clamp_region_to_monitor_truncates_overlap() {
        let (x, y, w, h, ox, oy) =
            clamp_region_to_monitor(0, 0, 1920, 1080, 1800, 1000, 300, 200).unwrap();
        assert_eq!((x, y), (1800, 1000));
        assert_eq!((w, h), (120, 80));
        assert_eq!((ox, oy), (1800.0, 1000.0));
    }

    #[test]
    fn test_cached_ocr_text_roundtrip() {
        update_cached_ocr_payload(
            "ocr_screen:test".to_string(),
            vec![1; 64],
            OcrCachePayload::Text("hello".to_string()),
        );
        let cached = get_cached_ocr_payload("ocr_screen:test", &[1; 64], 5000, 0.0);
        assert!(matches!(cached, Some(OcrCachePayload::Text(ref s)) if s == "hello"));
    }

    #[test]
    fn test_cached_ocr_matches_miss_on_different_key() {
        update_cached_ocr_payload(
            "ocr_find_text:test".to_string(),
            vec![2; 64],
            OcrCachePayload::Matches(vec![OcrMatch {
                text: "Hello".to_string(),
                x: 1.0, y: 2.0, width: 3.0, height: 4.0,
                center_x: 2.5, center_y: 4.0, confidence: 1.0,
            }]),
        );
        assert!(get_cached_ocr_payload("ocr_find_text:other", &[2; 64], 5000, 0.0).is_none());
    }
}
