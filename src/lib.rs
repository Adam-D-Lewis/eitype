//! eitype - A library for typing text using Emulated Input (EI) on Wayland
//!
//! This library provides a simple interface to connect to an EI server and emulate
//! keyboard input. It supports both portal-based connections (with user authorization)
//! and direct socket connections.
//!
//! # Example
//!
//! ```no_run
//! use eitype::{EiType, EiTypeConfig};
//!
//! // Connect via portal (will prompt for authorization on first use)
//! let mut typer = EiType::connect_portal(EiTypeConfig::default()).unwrap();
//!
//! // Type some text
//! typer.type_text("Hello, world!").unwrap();
//!
//! // Press special keys
//! typer.press_key("Return").unwrap();
//! ```

use log::{debug, error, info, trace, warn};
use reis::ei::{self, handshake::ContextType, keyboard::KeyState};
use reis::event::{DeviceCapability, EiEvent};
use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use thiserror::Error;
use xkbcommon::xkb;

#[cfg(feature = "python")]
use pyo3::prelude::*;

/// Global tokio runtime for portal connections.
/// Using a single runtime avoids issues with zbus/DBus connection state
/// being left in a bad state when a runtime is dropped.
static TOKIO_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

fn get_tokio_runtime() -> &'static tokio::runtime::Runtime {
    TOKIO_RUNTIME.get_or_init(|| {
        debug!("Creating global tokio runtime for portal connections");
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime")
    })
}

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur when using eitype
#[derive(Error, Debug)]
pub enum EiTypeError {
    /// Failed to connect to EI server
    #[error("Connection error: {0}")]
    Connection(String),

    /// Failed to load or parse keymap
    #[error("Keymap error: {0}")]
    Keymap(String),

    /// Unknown key name
    #[error("Unknown key: {0}")]
    UnknownKey(String),

    /// Error during typing operation
    #[error("Typing error: {0}")]
    Typing(String),

    /// No keyboard device found
    #[error("No keyboard device found")]
    NoKeyboard,

    /// Character not found in keymap
    #[error("Character not found in keymap: {0}")]
    CharNotFound(char),
}

// ============================================================================
// Configuration Types
// ============================================================================

/// Configuration for keyboard layout and typing behavior
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "python", pyclass)]
pub struct EiTypeConfig {
    /// XKB keyboard layout (e.g., "us", "de", "fr")
    pub layout: Option<String>,
    /// XKB keyboard variant (e.g., "dvorak", "colemak")
    pub variant: Option<String>,
    /// XKB keyboard model (e.g., "pc104", "pc105")
    pub model: Option<String>,
    /// XKB keyboard options (e.g., "ctrl:nocaps")
    pub options: Option<String>,
    /// Layout index to use when multiple layouts are available.
    /// `None` = auto-detect from desktop environment, falling back to 0.
    pub layout_index: Option<u32>,
    /// Delay between key events in milliseconds (default: 0)
    pub delay_ms: u64,
}

#[cfg(feature = "python")]
#[pymethods]
impl EiTypeConfig {
    #[new]
    #[pyo3(signature = (layout=None, variant=None, model=None, options=None, layout_index=None, delay_ms=0))]
    fn py_new(
        layout: Option<String>,
        variant: Option<String>,
        model: Option<String>,
        options: Option<String>,
        layout_index: Option<u32>,
        delay_ms: u64,
    ) -> Self {
        Self {
            layout,
            variant,
            model,
            options,
            layout_index,
            delay_ms,
        }
    }
}

impl EiTypeConfig {
    /// Create config from environment variables
    pub fn from_env() -> Self {
        Self {
            layout: std::env::var("XKB_DEFAULT_LAYOUT").ok(),
            variant: std::env::var("XKB_DEFAULT_VARIANT").ok(),
            model: std::env::var("XKB_DEFAULT_MODEL").ok(),
            options: std::env::var("XKB_DEFAULT_OPTIONS").ok(),
            layout_index: None,
            delay_ms: 0,
        }
    }

    /// Check if any XKB configuration is specified
    fn is_specified(&self) -> bool {
        self.layout.is_some()
            || self.variant.is_some()
            || self.model.is_some()
            || self.options.is_some()
    }
}

/// Actions that can be performed
#[derive(Debug, Clone)]
pub enum Action {
    /// Type a string of text
    Type(String),
    /// Press and release a special key (e.g., "Return", "Tab")
    Key(String),
    /// Hold a modifier key (will be released at the end)
    ModifierHold(String),
    /// Press and release a modifier key
    ModifierPress(String),
}

// ============================================================================
// Internal Utilities
// ============================================================================

/// Build a map of key names to keycodes (evdev codes)
fn build_key_to_keycode_map() -> HashMap<String, u32> {
    let mut map = HashMap::new();

    // Modifiers
    map.insert("shift".to_string(), 42);
    map.insert("lshift".to_string(), 42);
    map.insert("rshift".to_string(), 54);
    map.insert("ctrl".to_string(), 29);
    map.insert("control".to_string(), 29);
    map.insert("lctrl".to_string(), 29);
    map.insert("rctrl".to_string(), 97);
    map.insert("alt".to_string(), 56);
    map.insert("lalt".to_string(), 56);
    map.insert("ralt".to_string(), 100);
    map.insert("altgr".to_string(), 100);
    map.insert("super".to_string(), 125);
    map.insert("meta".to_string(), 125);
    map.insert("win".to_string(), 125);
    map.insert("lsuper".to_string(), 125);
    map.insert("rsuper".to_string(), 126);

    // Special keys
    map.insert("escape".to_string(), 1);
    map.insert("esc".to_string(), 1);
    map.insert("return".to_string(), 28);
    map.insert("enter".to_string(), 28);
    map.insert("tab".to_string(), 15);
    map.insert("backspace".to_string(), 14);
    map.insert("delete".to_string(), 111);
    map.insert("insert".to_string(), 110);
    map.insert("home".to_string(), 102);
    map.insert("end".to_string(), 107);
    map.insert("pageup".to_string(), 104);
    map.insert("pagedown".to_string(), 109);
    map.insert("space".to_string(), 57);
    map.insert("capslock".to_string(), 58);
    map.insert("numlock".to_string(), 69);
    map.insert("scrolllock".to_string(), 70);
    map.insert("print".to_string(), 99);
    map.insert("printscreen".to_string(), 99);
    map.insert("pause".to_string(), 119);
    map.insert("menu".to_string(), 127);

    // Arrow keys
    map.insert("up".to_string(), 103);
    map.insert("down".to_string(), 108);
    map.insert("left".to_string(), 105);
    map.insert("right".to_string(), 106);

    // Function keys
    for i in 1..=12 {
        map.insert(format!("f{}", i), 58 + i);
    }

    // Number keys (top row)
    map.insert("1".to_string(), 2);
    map.insert("2".to_string(), 3);
    map.insert("3".to_string(), 4);
    map.insert("4".to_string(), 5);
    map.insert("5".to_string(), 6);
    map.insert("6".to_string(), 7);
    map.insert("7".to_string(), 8);
    map.insert("8".to_string(), 9);
    map.insert("9".to_string(), 10);
    map.insert("0".to_string(), 11);

    // Letter keys
    let letters = "abcdefghijklmnopqrstuvwxyz";
    let letter_codes = [
        30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17,
        45, 21, 44,
    ];
    for (ch, code) in letters.chars().zip(letter_codes.iter()) {
        map.insert(ch.to_string(), *code);
    }

    map
}

/// Find the keycode for a character, and whether shift is needed
fn find_keycode_for_char(
    ch: char,
    keymap: &xkb::Keymap,
    layout_index: u32,
) -> Result<(u32, bool), EiTypeError> {
    let min_keycode: u32 = keymap.min_keycode().into();
    let max_keycode: u32 = keymap.max_keycode().into();

    for keycode_raw in min_keycode..=max_keycode {
        let keycode = xkb::Keycode::new(keycode_raw);
        let num_layouts = keymap.num_layouts_for_key(keycode);

        if layout_index < num_layouts {
            let num_levels = keymap.num_levels_for_key(keycode, layout_index);

            for level in 0..num_levels {
                let syms = keymap.key_get_syms_by_level(keycode, layout_index, level);

                for sym in syms {
                    let sym_raw: u32 = (*sym).into();
                    if let Some(sym_char) = keysym_to_char(sym_raw) {
                        if sym_char == ch {
                            let need_shift = level == 1;
                            let evdev_keycode = keycode_raw - 8;
                            return Ok((evdev_keycode, need_shift));
                        }
                    }
                }
            }
        }
    }

    Err(EiTypeError::CharNotFound(ch))
}

/// Convert an XKB keysym to a character
fn keysym_to_char(keysym: u32) -> Option<char> {
    if (0x20..=0x7e).contains(&keysym) {
        return char::from_u32(keysym);
    }
    if (0xa0..=0xff).contains(&keysym) {
        return char::from_u32(keysym);
    }
    if keysym >= 0x1000000 {
        return char::from_u32(keysym - 0x1000000);
    }
    match keysym {
        0xff0d => Some('\n'),
        0xff09 => Some('\t'),
        0x20 => Some(' '),
        _ => None,
    }
}

/// Get current timestamp in microseconds
fn get_timestamp() -> u64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let start = START.get_or_init(Instant::now);
    start.elapsed().as_micros() as u64
}

/// Try to detect the active keyboard layout index from the desktop environment.
///
/// Tries GNOME first, then KDE Plasma. Returns `None` if detection fails
/// (expected on unsupported desktops).
fn detect_active_layout_index() -> Option<u32> {
    if let Some(index) = detect_gnome_layout_index() {
        return Some(index);
    }
    if let Some(index) = detect_kde_layout_index() {
        return Some(index);
    }
    None
}

/// Detect active layout index on GNOME via gsettings.
fn detect_gnome_layout_index() -> Option<u32> {
    let output = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.input-sources", "current"])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            // Output is like "uint32 1"
            let index = stdout
                .trim()
                .rsplit_once(' ')
                .and_then(|(_, n)| n.parse::<u32>().ok());
            if let Some(i) = index {
                info!("Auto-detected GNOME active layout index: {}", i);
                return Some(i);
            }
            debug!("Failed to parse GNOME layout index from: {:?}", stdout.trim());
            None
        }
        Ok(out) => {
            debug!(
                "gsettings returned non-zero exit code: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
            None
        }
        Err(e) => {
            debug!("gsettings not available: {}", e);
            None
        }
    }
}

/// Detect active layout index on KDE Plasma via qdbus.
fn detect_kde_layout_index() -> Option<u32> {
    let output = std::process::Command::new("qdbus")
        .args([
            "org.kde.keyboard",
            "/Layouts",
            "org.kde.KeyboardLayouts.getLayout",
        ])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let index = stdout.trim().parse::<u32>().ok();
            if let Some(i) = index {
                info!("Auto-detected KDE active layout index: {}", i);
                return Some(i);
            }
            debug!("Failed to parse KDE layout index from: {:?}", stdout.trim());
            None
        }
        Ok(out) => {
            debug!(
                "qdbus returned non-zero exit code: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
            None
        }
        Err(e) => {
            debug!("qdbus not available: {}", e);
            None
        }
    }
}

// ============================================================================
// Connection Functions
// ============================================================================

/// Connect to EI via the XDG RemoteDesktop portal.
/// Returns the stream and optionally a new restore token for future sessions.
fn connect_via_portal(
    restore_token: Option<&str>,
) -> Result<(UnixStream, Option<String>), EiTypeError> {
    use ashpd::desktop::remote_desktop::{DeviceType, RemoteDesktop};
    use ashpd::desktop::PersistMode;

    info!("Connecting via XDG RemoteDesktop portal...");
    if restore_token.is_some() {
        info!("Using saved restore token for session persistence");
    }

    // Use global runtime to avoid DBus connection issues when runtime is dropped.
    // Creating a new runtime per connection causes zbus to leave stale state
    // that blocks subsequent connections.
    let rt = get_tokio_runtime();

    rt.block_on(async {
        let proxy = RemoteDesktop::new().await.map_err(|e| {
            EiTypeError::Connection(format!("Failed to create RemoteDesktop proxy: {}", e))
        })?;

        let session = proxy
            .create_session()
            .await
            .map_err(|e| EiTypeError::Connection(format!("Failed to create session: {}", e)))?;

        proxy
            .select_devices(
                &session,
                DeviceType::Keyboard.into(),
                restore_token,
                PersistMode::ExplicitlyRevoked,
            )
            .await
            .map_err(|e| EiTypeError::Connection(format!("Failed to select devices: {}", e)))?;

        let response = proxy
            .start(&session, None)
            .await
            .map_err(|e| EiTypeError::Connection(format!("Failed to start session: {}", e)))?
            .response()
            .map_err(|e| {
                EiTypeError::Connection(format!("Failed to get session response: {}", e))
            })?;

        let new_token = response.restore_token().map(|s| s.to_string());
        if new_token.is_some() {
            debug!("Received new restore token from portal");
        }

        let fd = proxy
            .connect_to_eis(&session)
            .await
            .map_err(|e| EiTypeError::Connection(format!("Failed to connect to EIS: {}", e)))?;

        let stream = UnixStream::from(fd);
        stream
            .set_nonblocking(true)
            .map_err(|e| EiTypeError::Connection(format!("Failed to set non-blocking: {}", e)))?;

        Ok((stream, new_token))
    })
}

/// Connect to EI via socket
fn connect_via_socket(path: &Path) -> Result<UnixStream, EiTypeError> {
    info!("Connecting to socket: {:?}", path);

    let stream = UnixStream::connect(path).map_err(|e| {
        EiTypeError::Connection(format!("Failed to connect to socket {:?}: {}", path, e))
    })?;

    stream
        .set_nonblocking(true)
        .map_err(|e| EiTypeError::Connection(format!("Failed to set non-blocking: {}", e)))?;

    Ok(stream)
}

// ============================================================================
// Main EiType Struct
// ============================================================================

/// Main interface for typing text via EI protocol
#[cfg_attr(feature = "python", pyclass(unsendable))]
pub struct EiType {
    connection: reis::event::Connection,
    device: reis::event::Device,
    keyboard: ei::Keyboard,
    keymap: Option<xkb::Keymap>,
    xkb_state: Option<xkb::State>,
    key_to_keycode: HashMap<String, u32>,
    delay: Duration,
    held_modifiers: Vec<u32>,
    sequence: u32,
    layout_index: u32,
    /// Track whether close() has been called to avoid double-close
    closed: bool,
}

impl EiType {
    /// Connect via the XDG RemoteDesktop portal (simple version, no token management)
    pub fn connect_portal(config: EiTypeConfig) -> Result<Self, EiTypeError> {
        let (eitype, _token) = Self::connect_portal_with_token(config, None)?;
        Ok(eitype)
    }

    /// Connect via the XDG RemoteDesktop portal with token support.
    ///
    /// If `restore_token` is provided and valid, the portal will skip the authorization dialog.
    /// Returns the EiType instance and optionally a new restore token to save for future use.
    pub fn connect_portal_with_token(
        config: EiTypeConfig,
        restore_token: Option<&str>,
    ) -> Result<(Self, Option<String>), EiTypeError> {
        let (stream, new_token) = connect_via_portal(restore_token)?;
        let eitype = Self::from_stream(stream, config)?;
        Ok((eitype, new_token))
    }

    /// Connect via a Unix socket (for testing or direct EIS connections)
    pub fn connect_socket(path: &Path, config: EiTypeConfig) -> Result<Self, EiTypeError> {
        let stream = connect_via_socket(path)?;
        Self::from_stream(stream, config)
    }

    /// Internal: create EiType from an already-connected stream
    fn from_stream(stream: UnixStream, config: EiTypeConfig) -> Result<Self, EiTypeError> {
        let context = ei::Context::new(stream)
            .map_err(|e| EiTypeError::Connection(format!("Failed to create EI context: {}", e)))?;

        info!("Performing handshake...");
        let (connection, mut event_iter) = context
            .handshake_blocking("eitype", ContextType::Sender)
            .map_err(|e| EiTypeError::Connection(format!("Handshake failed: {}", e)))?;

        info!("Connected! Waiting for devices...");

        // Process events until we get a keyboard device
        let mut result: Option<(reis::event::Device, ei::Keyboard)> = None;

        for event_result in &mut event_iter {
            let event = event_result
                .map_err(|e| EiTypeError::Connection(format!("Error processing event: {}", e)))?;
            trace!("Received event: {:?}", event);

            match event {
                EiEvent::Disconnected(disconnected) => {
                    let reason = disconnected.reason;
                    let explanation = &disconnected.explanation;
                    error!("Disconnected: {:?} - {}", reason, explanation);
                    return Err(EiTypeError::Connection(
                        "Disconnected from EI server".to_string(),
                    ));
                }

                EiEvent::SeatAdded(seat_added) => {
                    let seat = &seat_added.seat;
                    debug!("Seat added: {:?}", seat.name());
                    seat.bind_capabilities(&[DeviceCapability::Keyboard]);
                    connection
                        .flush()
                        .map_err(|e| EiTypeError::Connection(e.to_string()))?;
                }

                EiEvent::DeviceAdded(device_added) => {
                    let device = &device_added.device;
                    debug!("Device added: {:?}", device.name());
                }

                EiEvent::DeviceResumed(device_resumed) => {
                    let device = device_resumed.device.clone();
                    debug!("Device resumed: {:?}", device.name());

                    if let Some(keyboard) = device.interface::<ei::Keyboard>() {
                        info!("Keyboard device available: {:?}", device.name());
                        result = Some((device, keyboard));
                        break;
                    }
                }

                EiEvent::DevicePaused(paused) => {
                    debug!("Device paused: {:?}", paused.device.name());
                }

                EiEvent::DeviceRemoved(removed) => {
                    debug!("Device removed: {:?}", removed.device.name());
                }

                _ => {
                    trace!("Other event");
                }
            }
        }

        let (device, keyboard) = result.ok_or(EiTypeError::NoKeyboard)?;

        let layout_index = config.layout_index.unwrap_or_else(|| {
            detect_active_layout_index().unwrap_or_else(|| {
                info!("No active layout detected, defaulting to layout index 0");
                0
            })
        });
        info!("Using layout index: {}", layout_index);

        let mut eitype = Self {
            connection,
            device,
            keyboard,
            keymap: None,
            xkb_state: None,
            key_to_keycode: build_key_to_keycode_map(),
            delay: Duration::from_millis(config.delay_ms),
            held_modifiers: Vec::new(),
            sequence: 1,
            layout_index,
            closed: false,
        };

        // Setup keymap
        eitype.setup_keymap(&config)?;

        // Start emulating
        eitype.start_emulating()?;

        Ok(eitype)
    }

    fn setup_keymap(&mut self, config: &EiTypeConfig) -> Result<(), EiTypeError> {
        let xkb_context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);

        if config.is_specified() {
            let rules = "";
            let model = config.model.as_deref().unwrap_or("");
            let layout = config.layout.as_deref().unwrap_or("");
            let variant = config.variant.as_deref().unwrap_or("");
            let options = config.options.clone();

            info!(
                "Loading keymap from configuration: layout={}, variant={}, model={}",
                if layout.is_empty() {
                    "(default)"
                } else {
                    layout
                },
                if variant.is_empty() {
                    "(none)"
                } else {
                    variant
                },
                if model.is_empty() { "(default)" } else { model }
            );

            let keymap = xkb::Keymap::new_from_names(
                &xkb_context,
                rules,
                model,
                layout,
                variant,
                options,
                xkb::KEYMAP_COMPILE_NO_FLAGS,
            )
            .ok_or_else(|| {
                EiTypeError::Keymap("Failed to load keymap from configuration".to_string())
            })?;

            let state = xkb::State::new(&keymap);
            self.keymap = Some(keymap);
            self.xkb_state = Some(state);
            return Ok(());
        }

        // Try to use the keymap provided by the EI server
        if let Some(keymap_info) = self.device.keymap() {
            use std::os::fd::FromRawFd;
            use std::os::fd::IntoRawFd;
            let fd_dup = rustix::io::dup(&keymap_info.fd).map_err(|e| {
                EiTypeError::Keymap(format!("Failed to duplicate keymap fd: {}", e))
            })?;
            let owned_fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(fd_dup.into_raw_fd()) };

            let keymap = unsafe {
                xkb::Keymap::new_from_fd(
                    &xkb_context,
                    owned_fd,
                    keymap_info.size as usize,
                    xkb::KEYMAP_FORMAT_TEXT_V1,
                    xkb::KEYMAP_COMPILE_NO_FLAGS,
                )
            }
            .map_err(|e| EiTypeError::Keymap(format!("Failed to read keymap from fd: {}", e)))?
            .ok_or_else(|| EiTypeError::Keymap("Failed to compile keymap".to_string()))?;

            let state = xkb::State::new(&keymap);

            let num_layouts = keymap.num_layouts();
            if num_layouts > 0 {
                let layout_name = keymap.layout_get_name(0);
                info!(
                    "Keymap loaded from EI server: layout=\"{}\" ({} layout(s) available)",
                    layout_name, num_layouts
                );
                for i in 0..num_layouts {
                    debug!("  Layout {}: \"{}\"", i, keymap.layout_get_name(i));
                }
            } else {
                info!("Keymap loaded from EI server (no layout name available)");
            }

            self.keymap = Some(keymap);
            self.xkb_state = Some(state);
            return Ok(());
        }

        // Fallback: use system default keymap
        info!("Loading system default keymap");

        let keymap = xkb::Keymap::new_from_names(
            &xkb_context,
            "",
            "",
            "",
            "",
            None,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .ok_or_else(|| EiTypeError::Keymap("Failed to load system default keymap".to_string()))?;

        let state = xkb::State::new(&keymap);

        self.keymap = Some(keymap);
        self.xkb_state = Some(state);
        Ok(())
    }

    fn start_emulating(&mut self) -> Result<(), EiTypeError> {
        let serial = self.connection.serial();
        self.device.device().start_emulating(serial, self.sequence);
        self.sequence += 1;
        self.flush_with_retry()
    }

    fn stop_emulating(&mut self) -> Result<(), EiTypeError> {
        let serial = self.connection.serial();
        self.device.device().stop_emulating(serial);
        self.flush_with_retry()
    }

    fn send_frame(&self) -> Result<(), EiTypeError> {
        let serial = self.connection.serial();
        let timestamp = get_timestamp();
        self.device.device().frame(serial, timestamp);
        self.flush_with_retry()
    }

    /// Flush the connection with retry logic for EAGAIN (WouldBlock) errors.
    ///
    /// When the socket buffer is full (common with long text input), flush()
    /// returns EAGAIN. Instead of failing immediately, we wait for the buffer
    /// to drain and retry.
    fn flush_with_retry(&self) -> Result<(), EiTypeError> {
        const MAX_RETRIES: u32 = 50;
        const INITIAL_DELAY_MS: u64 = 1;
        const MAX_DELAY_MS: u64 = 100;

        let mut retries = 0;
        let mut delay_ms = INITIAL_DELAY_MS;

        loop {
            match self.connection.flush() {
                Ok(()) => return Ok(()),
                Err(e) => {
                    // Check if this is EAGAIN/EWOULDBLOCK (errno 11 on Linux)
                    // Use raw_os_error() to avoid rustix version mismatch issues
                    // (reis uses a different rustix version than our direct dependency)
                    let raw_errno = e.raw_os_error();
                    let is_would_block = raw_errno == 11; // EAGAIN == EWOULDBLOCK on Linux

                    if !is_would_block {
                        // Not a recoverable error, fail immediately
                        return Err(EiTypeError::Typing(e.to_string()));
                    }

                    retries += 1;
                    if retries > MAX_RETRIES {
                        return Err(EiTypeError::Typing(format!(
                            "Socket buffer full after {} retries: {}",
                            MAX_RETRIES, e
                        )));
                    }

                    trace!(
                        "Socket buffer full (EAGAIN), waiting {}ms before retry {}/{}",
                        delay_ms,
                        retries,
                        MAX_RETRIES
                    );

                    std::thread::sleep(Duration::from_millis(delay_ms));

                    // Exponential backoff with cap
                    delay_ms = (delay_ms * 2).min(MAX_DELAY_MS);
                }
            }
        }
    }

    fn press_key_internal(&self, keycode: u32) -> Result<(), EiTypeError> {
        trace!("Pressing key: {}", keycode);
        self.keyboard.key(keycode, KeyState::Press);
        self.send_frame()?;
        Ok(())
    }

    fn release_key_internal(&self, keycode: u32) -> Result<(), EiTypeError> {
        trace!("Releasing key: {}", keycode);
        self.keyboard.key(keycode, KeyState::Released);
        self.send_frame()?;
        Ok(())
    }

    fn tap_key_internal(&self, keycode: u32) -> Result<(), EiTypeError> {
        self.press_key_internal(keycode)?;
        if !self.delay.is_zero() {
            std::thread::sleep(self.delay);
        }
        self.release_key_internal(keycode)?;
        if !self.delay.is_zero() {
            std::thread::sleep(self.delay);
        }
        Ok(())
    }

    fn type_char(&self, ch: char) -> Result<(), EiTypeError> {
        trace!("Typing character: {:?}", ch);

        if let Some(keymap) = &self.keymap {
            let (keycode, need_shift) = find_keycode_for_char(ch, keymap, self.layout_index)?;

            if need_shift {
                let shift_keycode = self.key_to_keycode.get("shift").copied().unwrap_or(42);
                self.press_key_internal(shift_keycode)?;
            }

            self.tap_key_internal(keycode)?;

            if need_shift {
                let shift_keycode = self.key_to_keycode.get("shift").copied().unwrap_or(42);
                self.release_key_internal(shift_keycode)?;
            }
        } else {
            // Fallback when no keymap: use hardcoded QWERTY map
            let ch_lower = ch.to_ascii_lowercase();
            if let Some(&keycode) = self.key_to_keycode.get(&ch_lower.to_string()) {
                let need_shift = ch.is_ascii_uppercase();

                if need_shift {
                    let shift_keycode = self.key_to_keycode.get("shift").copied().unwrap_or(42);
                    self.press_key_internal(shift_keycode)?;
                }

                self.tap_key_internal(keycode)?;

                if need_shift {
                    let shift_keycode = self.key_to_keycode.get("shift").copied().unwrap_or(42);
                    self.release_key_internal(shift_keycode)?;
                }
            } else {
                warn!("Could not find keycode for character: {:?}", ch);
                return Err(EiTypeError::CharNotFound(ch));
            }
        }

        Ok(())
    }

    /// Type a string of text
    pub fn type_text(&self, text: &str) -> Result<(), EiTypeError> {
        debug!("Typing text: {:?}", text);
        for ch in text.chars() {
            self.type_char(ch)?;
        }
        Ok(())
    }

    /// Press and release a special key (e.g., "Return", "Tab", "Escape")
    pub fn press_key(&self, key_name: &str) -> Result<(), EiTypeError> {
        let keycode = self
            .key_to_keycode
            .get(&key_name.to_lowercase())
            .copied()
            .ok_or_else(|| EiTypeError::UnknownKey(key_name.to_string()))?;

        debug!("Pressing special key: {} (keycode {})", key_name, keycode);
        self.tap_key_internal(keycode)
    }

    /// Hold a modifier key (will be released when release_modifiers is called)
    pub fn hold_modifier(&mut self, mod_name: &str) -> Result<(), EiTypeError> {
        let keycode = self
            .key_to_keycode
            .get(&mod_name.to_lowercase())
            .copied()
            .ok_or_else(|| EiTypeError::UnknownKey(mod_name.to_string()))?;

        debug!("Holding modifier: {} (keycode {})", mod_name, keycode);
        self.press_key_internal(keycode)?;
        self.held_modifiers.push(keycode);
        Ok(())
    }

    /// Press and release a modifier key (like a regular key press)
    pub fn press_modifier(&self, mod_name: &str) -> Result<(), EiTypeError> {
        let keycode = self
            .key_to_keycode
            .get(&mod_name.to_lowercase())
            .copied()
            .ok_or_else(|| EiTypeError::UnknownKey(mod_name.to_string()))?;

        debug!("Pressing modifier: {} (keycode {})", mod_name, keycode);
        self.tap_key_internal(keycode)
    }

    /// Release all held modifiers
    pub fn release_modifiers(&mut self) -> Result<(), EiTypeError> {
        for keycode in self.held_modifiers.drain(..).rev().collect::<Vec<_>>() {
            debug!("Releasing held modifier keycode {}", keycode);
            self.release_key_internal(keycode)?;
        }
        Ok(())
    }

    /// Execute a sequence of actions
    pub fn execute_actions(&mut self, actions: &[Action]) -> Result<(), EiTypeError> {
        info!("Executing {} actions", actions.len());

        for action in actions {
            match action {
                Action::Type(text) => {
                    self.type_text(text)?;
                }
                Action::Key(key_name) => {
                    self.press_key(key_name)?;
                }
                Action::ModifierHold(mod_name) => {
                    self.hold_modifier(mod_name)?;
                }
                Action::ModifierPress(mod_name) => {
                    self.press_modifier(mod_name)?;
                }
            }
        }

        // Release any held modifiers
        self.release_modifiers()?;

        Ok(())
    }

    /// Explicitly close the connection and release all resources.
    ///
    /// This method should be called when you're done with the EiType instance,
    /// especially before attempting to create a new connection. While the Drop
    /// implementation will clean up automatically, calling close() explicitly
    /// ensures proper cleanup and avoids potential issues with reconnection.
    ///
    /// After calling close(), this instance should not be used anymore.
    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;

        debug!("Closing EiType connection");

        // Release any held modifiers
        let _ = self.release_modifiers();

        // Stop emulating
        let _ = self.stop_emulating();

        // Send disconnect request to the EI server
        // This tells the server we're intentionally disconnecting
        self.connection.connection().disconnect();

        // Flush to ensure the disconnect message is sent
        let _ = self.connection.flush();

        debug!("EiType connection closed");
    }
}

impl Drop for EiType {
    fn drop(&mut self) {
        // Use close() which handles all cleanup including EI disconnect
        self.close();
    }
}

// ============================================================================
// Python Bindings
// ============================================================================

#[cfg(feature = "python")]
#[pymethods]
impl EiType {
    /// Connect via the XDG RemoteDesktop portal (simple version)
    #[staticmethod]
    #[pyo3(signature = (config=None))]
    fn py_connect_portal(config: Option<EiTypeConfig>) -> PyResult<Self> {
        let config = config.unwrap_or_default();
        Self::connect_portal(config)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Connect via the XDG RemoteDesktop portal with token support
    #[staticmethod]
    #[pyo3(signature = (restore_token=None, config=None))]
    fn py_connect_portal_with_token(
        restore_token: Option<&str>,
        config: Option<EiTypeConfig>,
    ) -> PyResult<(Self, Option<String>)> {
        let config = config.unwrap_or_default();
        Self::connect_portal_with_token(config, restore_token)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Connect via a Unix socket
    #[staticmethod]
    #[pyo3(signature = (path, config=None))]
    fn py_connect_socket(path: &str, config: Option<EiTypeConfig>) -> PyResult<Self> {
        let config = config.unwrap_or_default();
        Self::connect_socket(Path::new(path), config)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Type a string of text
    #[pyo3(name = "type_text")]
    fn py_type_text(&self, text: &str) -> PyResult<()> {
        self.type_text(text)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Press and release a special key
    #[pyo3(name = "press_key")]
    fn py_press_key(&self, key_name: &str) -> PyResult<()> {
        self.press_key(key_name)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Hold a modifier key
    #[pyo3(name = "hold_modifier")]
    fn py_hold_modifier(&mut self, mod_name: &str) -> PyResult<()> {
        self.hold_modifier(mod_name)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Press and release a modifier key
    #[pyo3(name = "press_modifier")]
    fn py_press_modifier(&self, mod_name: &str) -> PyResult<()> {
        self.press_modifier(mod_name)
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Release all held modifiers
    #[pyo3(name = "release_modifiers")]
    fn py_release_modifiers(&mut self) -> PyResult<()> {
        self.release_modifiers()
            .map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e.to_string()))
    }

    /// Close the connection and release all resources.
    ///
    /// This method should be called when you're done with the EiType instance,
    /// especially before attempting to create a new connection. While the
    /// destructor will clean up automatically, calling close() explicitly
    /// ensures proper cleanup and avoids potential issues with reconnection.
    ///
    /// After calling close(), this instance should not be used anymore.
    #[pyo3(name = "close")]
    fn py_close(&mut self) {
        self.close();
    }

    /// Context manager entry - returns self for use with `with` statement.
    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    /// Context manager exit - closes the connection.
    ///
    /// This ensures proper cleanup when used with `with` statements:
    /// ```python
    /// with EiType.connect_portal() as typer:
    ///     typer.type_text("Hello!")
    /// # Connection is automatically closed here
    /// ```
    #[pyo3(signature = (_exc_type=None, _exc_value=None, _traceback=None))]
    fn __exit__(
        &mut self,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_value: Option<&Bound<'_, PyAny>>,
        _traceback: Option<&Bound<'_, PyAny>>,
    ) -> bool {
        self.close();
        false // Don't suppress exceptions
    }
}

#[cfg(feature = "python")]
#[pymodule]
fn eitype(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<EiType>()?;
    m.add_class::<EiTypeConfig>()?;
    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_key_to_keycode_map_modifiers() {
        let map = build_key_to_keycode_map();
        assert_eq!(map.get("shift"), Some(&42));
        assert_eq!(map.get("ctrl"), Some(&29));
        assert_eq!(map.get("alt"), Some(&56));
        assert_eq!(map.get("super"), Some(&125));
    }

    #[test]
    fn test_build_key_to_keycode_map_special_keys() {
        let map = build_key_to_keycode_map();
        assert_eq!(map.get("escape"), Some(&1));
        assert_eq!(map.get("return"), Some(&28));
        assert_eq!(map.get("tab"), Some(&15));
        assert_eq!(map.get("space"), Some(&57));
    }

    #[test]
    fn test_build_key_to_keycode_map_letters() {
        let map = build_key_to_keycode_map();
        assert_eq!(map.get("a"), Some(&30));
        assert_eq!(map.get("z"), Some(&44));
    }

    #[test]
    fn test_keysym_to_char_ascii() {
        assert_eq!(keysym_to_char(0x20), Some(' '));
        assert_eq!(keysym_to_char(0x41), Some('A'));
        assert_eq!(keysym_to_char(0x61), Some('a'));
    }

    #[test]
    fn test_keysym_to_char_unicode() {
        assert_eq!(keysym_to_char(0x1000000 + 0x00e9), Some('é'));
    }

    #[test]
    fn test_config_default() {
        let config = EiTypeConfig::default();
        assert!(config.layout.is_none());
        assert!(config.layout_index.is_none());
        assert!(!config.is_specified());
    }

    #[test]
    fn test_config_is_specified() {
        let mut config = EiTypeConfig::default();
        assert!(!config.is_specified());

        config.layout = Some("us".to_string());
        assert!(config.is_specified());
    }

    #[test]
    fn test_get_timestamp_monotonic() {
        let t1 = get_timestamp();
        std::thread::sleep(std::time::Duration::from_millis(1));
        let t2 = get_timestamp();
        assert!(t2 >= t1);
    }

    #[test]
    fn test_exponential_backoff_calculation() {
        // Verify the backoff formula used in flush_with_retry():
        // delay_ms = (delay_ms * 2).min(MAX_DELAY_MS)
        let max_delay_ms: u64 = 100;
        let mut delay_ms: u64 = 1;

        // Should double each time until capped at 100ms
        let expected_delays = [2, 4, 8, 16, 32, 64, 100, 100];
        for expected in expected_delays {
            delay_ms = (delay_ms * 2).min(max_delay_ms);
            assert_eq!(delay_ms, expected);
        }
    }

    #[test]
    fn test_eagain_errno_value() {
        // Document the expected errno for EAGAIN on Linux.
        // flush_with_retry() checks raw_os_error() == 11 to detect
        // when the socket buffer is full and we should retry.
        assert_eq!(libc::EAGAIN, 11);
        // On Linux, EWOULDBLOCK is the same as EAGAIN
        assert_eq!(libc::EWOULDBLOCK, libc::EAGAIN);
    }
}
