use super::*;
use super::trace::*;

#[test]
fn test_parse_key_combo_single() {
    let (mods, key) = parse_key_combo("enter").unwrap();
    assert!(mods.is_empty());
    assert!(matches!(key, enigo::Key::Return));
}

#[test]
fn test_parse_key_combo_with_modifier() {
    let (mods, key) = parse_key_combo("ctrl+c").unwrap();
    assert_eq!(mods.len(), 1);
    assert!(matches!(mods[0], enigo::Key::Control));
    assert!(matches!(key, enigo::Key::Unicode('c')));
}

#[test]
fn test_parse_key_combo_multiple_modifiers() {
    let (mods, key) = parse_key_combo("ctrl+shift+s").unwrap();
    assert_eq!(mods.len(), 2);
    assert!(matches!(mods[0], enigo::Key::Control));
    assert!(matches!(mods[1], enigo::Key::Shift));
    assert!(matches!(key, enigo::Key::Unicode('s')));
}

#[test]
fn test_parse_key_combo_fkey() {
    let (mods, key) = parse_key_combo("alt+f4").unwrap();
    assert_eq!(mods.len(), 1);
    assert!(matches!(mods[0], enigo::Key::Alt));
    assert!(matches!(key, enigo::Key::F4));
}

#[test]
fn test_parse_key_combo_unknown() {
    assert!(parse_key_combo("ctrl+unknownkey").is_err());
}

#[test]
fn test_str_to_key_special() {
    assert!(matches!(str_to_key("tab"), Ok(enigo::Key::Tab)));
    assert!(matches!(str_to_key("escape"), Ok(enigo::Key::Escape)));
    assert!(matches!(str_to_key("backspace"), Ok(enigo::Key::Backspace)));
    assert!(matches!(str_to_key("space"), Ok(enigo::Key::Space)));
}

#[test]
fn test_str_to_key_char() {
    assert!(matches!(str_to_key("a"), Ok(enigo::Key::Unicode('a'))));
    assert!(matches!(str_to_key("1"), Ok(enigo::Key::Unicode('1'))));
}

#[test]
#[ignore = "requires display"]
fn test_validate_coordinates_on_screen() {
    // (0,0) should always be valid — it's the top-left of the primary monitor
    assert!(validate_coordinates(0, 0).is_ok());
}

#[test]
fn test_validate_coordinates_off_screen() {
    // Extremely negative coords should be invalid
    assert!(validate_coordinates(-99999, -99999).is_err());
}

#[test]
#[ignore = "requires display"]
fn test_with_enigo_caches_instance() {
    // First call creates the instance
    let r1 = with_enigo(|_e| Ok::<_, String>(42));
    assert_eq!(r1.unwrap(), 42);
    // Second call reuses it (no error from re-init)
    let r2 = with_enigo(|_e| Ok::<_, String>(99));
    assert_eq!(r2.unwrap(), 99);
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
#[test]
fn test_dpi_scaling_noop_when_false() {
    let (x, y) = apply_dpi_scaling(100, 200, false);
    assert_eq!((x, y), (100, 200));
}

#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
#[test]
fn test_dpi_scaling_applies_when_true() {
    let (x, y) = apply_dpi_scaling(100, 200, true);
    // At any DPI >= 96, the scaled values should be >= input values
    assert!(x >= 100);
    assert!(y >= 200);
}

#[test]
fn test_screenshot_cache_empty() {
    // Cache starts empty
    assert!(get_cached_screenshot(1000).is_none());
}

#[test]
fn test_screenshot_cache_roundtrip() {
    use std::sync::Arc;
    let raw = vec![1, 2, 3, 4];
    let png = vec![5, 6, 7, 8];
    update_screenshot_cache(raw.clone(), png.clone());
    let cached = get_cached_screenshot(5000);
    assert_eq!(cached, Some((Arc::new(raw), Arc::new(png))));
}

#[test]
fn test_pixel_diff_identical() {
    let data = vec![100u8; 256 * 4];
    assert!(pixel_diff_pct(&data, &data) < 0.01);
}

#[test]
fn test_pixel_diff_completely_different() {
    let a = vec![0u8; 256 * 4];
    let b = vec![255u8; 256 * 4];
    assert!(pixel_diff_pct(&a, &b) > 99.0);
}

#[test]
fn test_pixel_diff_different_lengths() {
    let a = vec![0u8; 100];
    let b = vec![0u8; 200];
    assert_eq!(pixel_diff_pct(&a, &b), 100.0);
}

#[test]
fn test_classify_desktop_result_status() {
    assert_eq!(classify_desktop_result_status("Pressed: enter"), "completed");
    assert_eq!(classify_desktop_result_status("Timeout: no window"), "timed_out");
    assert_eq!(classify_desktop_result_status("Error [ocr_screen]: Operation timed out"), "timed_out");
    assert_eq!(classify_desktop_result_status("Error [ocr_screen]: Operation cancelled"), "cancelled");
    assert_eq!(classify_desktop_result_status("Error [click_screen]: bad coordinate"), "error");
    assert_eq!(
        classify_desktop_result_status(
            "Pressed: enter. Verification failed: expected screen change >= 0.50%, observed 0.00% after 1200ms."
        ),
        "verification_failed"
    );
}

#[test]
fn test_summarize_value_for_trace_truncates_long_strings() {
    let value = serde_json::json!({
        "text": "x".repeat(250),
        "nested": ["short", "y".repeat(250)]
    });

    let summarized = summarize_value_for_trace(&value);
    let text = summarized.get("text").and_then(|v| v.as_str()).unwrap();
    assert!(text.len() <= 203);
    assert!(text.ends_with("..."));
}

#[test]
fn test_parse_float_handles_strings() {
    assert_eq!(parse_float(&serde_json::json!(1.5)), Some(1.5));
    assert_eq!(parse_float(&serde_json::json!("2.25")), Some(2.25));
    assert_eq!(parse_float(&serde_json::json!("nope")), None);
}

#[test]
fn test_normalize_verification_region_rejects_zero_size() {
    assert_eq!(normalize_verification_region(10, 10, 0, 10), None);
    assert_eq!(normalize_verification_region(10, 10, 10, 0), None);
}

#[test]
fn test_action_verification_region_from_args() {
    use super::trace::action_verification_region_from_args;
    let args = serde_json::json!({
        "verify_x": 100,
        "verify_y": 200,
        "verify_width": 300,
        "verify_height": 400
    });
    assert_eq!(
        action_verification_region_from_args(&args),
        Some(VerificationRegion {
            x: 100,
            y: 200,
            width: 300,
            height: 400
        })
    );
}

// ─── Round 6: tool_error / tool_not_supported format tests ───────────

#[test]
fn test_tool_error_format() {
    let r = tool_error("click_screen", "bad coordinate");
    assert_eq!(r.text, "Error [click_screen]: bad coordinate");
    assert!(r.images.is_empty());
}

#[test]
fn test_tool_error_with_format_string() {
    let r = tool_error("ocr_screen", format!("monitor {} out of range", 3));
    assert_eq!(r.text, "Error [ocr_screen]: monitor 3 out of range");
}

#[test]
fn test_tool_not_supported_format() {
    let r = tool_not_supported("handle_dialog");
    assert_eq!(r.text, "Error [handle_dialog]: not available on this platform");
}

// ─── Round 6: validated_monitors tests ───────────────────────────────

#[test]
#[ignore = "requires display"]
fn test_validated_monitors_index_zero_ok() {
    // Index 0 should always succeed on a machine with a monitor
    let result = validated_monitors("test_tool", 0);
    assert!(result.is_ok());
    let monitors = result.unwrap();
    assert!(!monitors.is_empty());
}

#[test]
#[ignore = "requires display"]
fn test_validated_monitors_out_of_range() {
    let result = validated_monitors("test_tool", 999);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.text.contains("monitor 999 out of range"));
    assert!(err.text.starts_with("Error [test_tool]:"));
}

// ─── Round 6: verify_text in prepare_screen_verification ─────────────

#[test]
fn test_prepare_verification_no_flags_returns_none() {
    let args = serde_json::json!({});
    let result = prepare_screen_verification(&args, None);
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
}

#[test]
#[ignore = "requires display"]
fn test_prepare_verification_verify_text_enables_verification() {
    // verify_text alone should enable verification (no verify_screen_change needed)
    let args = serde_json::json!({
        "verify_text": "Hello World"
    });
    let result = prepare_screen_verification(&args, None);
    assert!(result.is_ok());
    let ctx = result.unwrap();
    assert!(ctx.is_some(), "verify_text should enable verification context");
    let ctx = ctx.unwrap();
    assert_eq!(ctx.expected_text, Some("Hello World".to_string()));
}

#[test]
#[ignore = "requires display"]
fn test_prepare_verification_screen_change_without_text() {
    let args = serde_json::json!({
        "verify_screen_change": true
    });
    let result = prepare_screen_verification(&args, None);
    assert!(result.is_ok());
    let ctx = result.unwrap();
    assert!(ctx.is_some(), "verify_screen_change=true should enable verification");
    assert_eq!(ctx.unwrap().expected_text, None);
}

// ─── Round 6: classify_desktop_result_status error format ────────────

#[test]
fn test_classify_result_status_new_tool_error_format() {
    // Verify the standardized Error [tool]: msg format is correctly classified
    assert_eq!(
        classify_desktop_result_status("Error [smart_wait]: 'text' is required when mode is 'all'"),
        "error"
    );
    assert_eq!(
        classify_desktop_result_status("Error [click_and_verify]: 'click_text' is required"),
        "error"
    );
}

// ─── Round 6: dispatch covers new Round 6 tools ──────────────────────

#[test]
fn test_dispatch_smart_wait_exists() {
    let dummy = serde_json::json!({});
    assert!(dispatch_desktop_tool("smart_wait", &dummy).is_some());
}

#[test]
fn test_dispatch_click_and_verify_exists() {
    let dummy = serde_json::json!({});
    assert!(dispatch_desktop_tool("click_and_verify", &dummy).is_some());
}

// ─── Round 7: parse_timeout ─────────────────────────────────────────

#[test]
fn test_parse_timeout_default() {
    let args = serde_json::json!({});
    let dur = parse_timeout(&args);
    assert_eq!(dur, DEFAULT_THREAD_TIMEOUT);
}

#[test]
fn test_parse_timeout_custom() {
    let args = serde_json::json!({"timeout_ms": 5000});
    let dur = parse_timeout(&args);
    assert_eq!(dur, std::time::Duration::from_millis(5000));
}

#[test]
fn test_parse_timeout_clamp_low() {
    let args = serde_json::json!({"timeout_ms": 100});
    let dur = parse_timeout(&args);
    assert_eq!(dur, std::time::Duration::from_millis(1000));
}

#[test]
fn test_parse_timeout_clamp_high() {
    let args = serde_json::json!({"timeout_ms": 999999});
    let dur = parse_timeout(&args);
    assert_eq!(dur, std::time::Duration::from_millis(60000));
}

// ─── Round 7: cached_monitors ───────────────────────────────────────

#[test]
#[ignore = "requires display"]
fn test_cached_monitors_returns_list() {
    let result = cached_monitors();
    // Should succeed on any system with at least one display
    assert!(result.is_ok());
    assert!(!result.unwrap().is_empty());
}

#[test]
#[ignore = "requires display"]
fn test_cached_monitors_second_call_uses_cache() {
    // First call populates cache
    let _ = cached_monitors();
    // Second call should also succeed (from cache)
    let result = cached_monitors();
    assert!(result.is_ok());
}
