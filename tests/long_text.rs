//! Integration tests for typing long text.
//!
//! These tests require a running Wayland desktop with EI support (e.g., GNOME on Wayland).
//! They are behind a feature flag because they:
//! - Require user authorization via the portal dialog (first run only)
//! - Actually type text into the focused window
//! - Will fail in CI environments without a display
//!
//! To run these tests:
//! ```sh
//! cargo test --features wayland-integration-tests
//! ```
//!
//! Before running, open a text editor and ensure it has focus so the typed
//! text goes somewhere visible.

#![cfg(feature = "wayland-integration-tests")]

use eitype::{EiType, EiTypeConfig};

/// Test typing 500+ characters to exercise the EAGAIN retry logic.
///
/// This test verifies the fix for issue #5: when typing long text, the
/// socket buffer can fill up and return EAGAIN. The flush_with_retry()
/// function should handle this by retrying with exponential backoff.
#[test]
fn test_type_500_chars() {
    // Generate 500+ characters of test text
    let test_text = "The quick brown fox jumps over the lazy dog. "
        .repeat(15); // 45 chars * 15 = 675 characters

    assert!(
        test_text.len() >= 500,
        "Test text should be at least 500 characters, got {}",
        test_text.len()
    );

    println!("Connecting to portal...");
    let typer = EiType::connect_portal(EiTypeConfig::default())
        .expect("Failed to connect to portal");

    println!("Typing {} characters...", test_text.len());
    typer
        .type_text(&test_text)
        .expect("Failed to type long text - EAGAIN retry may have failed");

    println!("Successfully typed {} characters!", test_text.len());
}

/// Test typing 1000+ characters - a more strenuous test.
#[test]
fn test_type_1000_chars() {
    let test_text = "abcdefghijklmnopqrstuvwxyz ABCDEFGHIJKLMNOPQRSTUVWXYZ 0123456789 "
        .repeat(20); // 66 chars * 20 = 1320 characters

    assert!(
        test_text.len() >= 1000,
        "Test text should be at least 1000 characters, got {}",
        test_text.len()
    );

    println!("Connecting to portal...");
    let typer = EiType::connect_portal(EiTypeConfig::default())
        .expect("Failed to connect to portal");

    println!("Typing {} characters...", test_text.len());
    typer
        .type_text(&test_text)
        .expect("Failed to type long text - EAGAIN retry may have failed");

    println!("Successfully typed {} characters!", test_text.len());
}

/// Test that simulates the original issue scenario: typing transcription-length text.
///
/// The original issue occurred with 622 characters from a 60+ second speech
/// transcription. This test uses a similar length.
#[test]
fn test_type_transcription_length() {
    // Simulate a realistic transcription with varied content
    let test_text = concat!(
        "This is a test of the emergency broadcast system. ",
        "In the event of an actual emergency, you would be instructed where to tune for news and official information. ",
        "The quick brown fox jumps over the lazy dog. ",
        "Pack my box with five dozen liquor jugs. ",
        "How vexingly quick daft zebras jump! ",
        "The five boxing wizards jump quickly. ",
        "Sphinx of black quartz, judge my vow. ",
        "Two driven jocks help fax my big quiz. ",
        "The jay, pig, fox, zebra and my wolves quack! ",
        "Sympathizing would fix Quaker objectives. ",
        "A wizard's job is to vex chumps quickly in fog. ",
        "Watch Jeopardy, Alex Trebek's fun TV quiz game. ",
        "By Jove, my quick study of lexicography won a prize! ",
    );

    // Should be around 620+ characters, similar to the original issue
    println!("Test text length: {} characters", test_text.len());
    assert!(
        test_text.len() >= 600,
        "Test text should be at least 600 characters to match issue scenario, got {}",
        test_text.len()
    );

    println!("Connecting to portal...");
    let typer = EiType::connect_portal(EiTypeConfig::default())
        .expect("Failed to connect to portal");

    println!("Typing {} characters (transcription-length test)...", test_text.len());
    typer
        .type_text(test_text)
        .expect("Failed to type transcription-length text - EAGAIN retry may have failed");

    println!("Successfully typed {} characters!", test_text.len());
}