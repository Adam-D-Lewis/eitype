//! eitype - A wtype-like CLI tool for typing text using Emulated Input (EI) on Wayland
//!
//! This tool connects to an EI server (either directly via socket or through the XDG portal)
//! and emulates keyboard input to type text.

use anyhow::{anyhow, bail, Context as AnyhowContext, Result};
use clap::Parser;
use log::{debug, error, info, trace, warn};
use reis::ei::{self, handshake::ContextType, keyboard::KeyState};
use reis::event::{DeviceCapability, EiEvent};
use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};
use xkbcommon::xkb;

/// A wtype-like tool for typing text using Emulated Input (EI) protocol
#[derive(Parser, Debug)]
#[command(name = "eitype", version, about, long_about = None)]
struct Args {
    /// Text to type (can be specified multiple times)
    #[arg(value_name = "TEXT")]
    text: Vec<String>,

    /// Delay between key events in milliseconds
    #[arg(short = 'd', long, default_value = "0", value_name = "MS")]
    delay: u64,

    /// Press a special key (e.g., return, tab, escape, backspace)
    #[arg(short = 'k', long = "key", value_name = "KEY")]
    keys: Vec<String>,

    /// Hold a modifier key (e.g., shift, ctrl, alt, super)
    #[arg(short = 'M', long = "mod", value_name = "MOD")]
    modifiers: Vec<String>,

    /// Press and release a modifier key
    #[arg(short = 'P', long = "press-mod", value_name = "MOD")]
    press_modifiers: Vec<String>,

    /// Use XDG RemoteDesktop portal for connection
    #[arg(short = 'p', long)]
    portal: bool,

    /// Socket path (defaults to LIBEI_SOCKET environment variable)
    #[arg(short = 's', long, value_name = "PATH")]
    socket: Option<String>,

    /// XKB keyboard layout (e.g., "us", "de", "fr"). Overrides XKB_DEFAULT_LAYOUT env var.
    #[arg(short = 'l', long, value_name = "LAYOUT")]
    layout: Option<String>,

    /// XKB keyboard variant (e.g., "dvorak", "colemak"). Overrides XKB_DEFAULT_VARIANT env var.
    #[arg(long, value_name = "VARIANT")]
    variant: Option<String>,

    /// XKB keyboard model (e.g., "pc104", "pc105"). Overrides XKB_DEFAULT_MODEL env var.
    #[arg(long, value_name = "MODEL")]
    model: Option<String>,

    /// XKB keyboard options (e.g., "ctrl:nocaps"). Overrides XKB_DEFAULT_OPTIONS env var.
    #[arg(long, value_name = "OPTIONS")]
    options: Option<String>,

    /// Verbose output
    #[arg(short = 'v', long, action = clap::ArgAction::Count)]
    verbose: u8,
}

/// XKB keyboard configuration
#[derive(Debug, Clone, Default)]
struct XkbConfig {
    rules: Option<String>,
    model: Option<String>,
    layout: Option<String>,
    variant: Option<String>,
    options: Option<String>,
}

impl XkbConfig {
    /// Create XkbConfig from CLI args and environment variables
    /// CLI args take precedence over environment variables
    fn from_args_and_env(args: &Args) -> Self {
        Self {
            rules: std::env::var("XKB_DEFAULT_RULES").ok(),
            model: args.model.clone().or_else(|| std::env::var("XKB_DEFAULT_MODEL").ok()),
            layout: args.layout.clone().or_else(|| std::env::var("XKB_DEFAULT_LAYOUT").ok()),
            variant: args.variant.clone().or_else(|| std::env::var("XKB_DEFAULT_VARIANT").ok()),
            options: args.options.clone().or_else(|| std::env::var("XKB_DEFAULT_OPTIONS").ok()),
        }
    }

    /// Check if any XKB configuration is specified
    fn is_specified(&self) -> bool {
        self.rules.is_some() || self.model.is_some() || self.layout.is_some()
            || self.variant.is_some() || self.options.is_some()
    }
}

/// Actions to perform after connection is established
#[derive(Debug, Clone)]
enum Action {
    Type(String),
    Key(String),
    ModifierHold(String),
    ModifierPress(String),
}

/// Build a map of key names to keycodes (evdev codes)
fn build_key_to_keycode_map() -> HashMap<String, u32> {
    let mut map = HashMap::new();

    // Modifiers
    map.insert("shift".to_string(), 42);      // KEY_LEFTSHIFT
    map.insert("lshift".to_string(), 42);     // KEY_LEFTSHIFT
    map.insert("rshift".to_string(), 54);     // KEY_RIGHTSHIFT
    map.insert("ctrl".to_string(), 29);       // KEY_LEFTCTRL
    map.insert("control".to_string(), 29);    // KEY_LEFTCTRL
    map.insert("lctrl".to_string(), 29);      // KEY_LEFTCTRL
    map.insert("rctrl".to_string(), 97);      // KEY_RIGHTCTRL
    map.insert("alt".to_string(), 56);        // KEY_LEFTALT
    map.insert("lalt".to_string(), 56);       // KEY_LEFTALT
    map.insert("ralt".to_string(), 100);      // KEY_RIGHTALT
    map.insert("altgr".to_string(), 100);     // KEY_RIGHTALT
    map.insert("super".to_string(), 125);     // KEY_LEFTMETA
    map.insert("meta".to_string(), 125);      // KEY_LEFTMETA
    map.insert("win".to_string(), 125);       // KEY_LEFTMETA
    map.insert("lsuper".to_string(), 125);    // KEY_LEFTMETA
    map.insert("rsuper".to_string(), 126);    // KEY_RIGHTMETA

    // Special keys
    map.insert("escape".to_string(), 1);      // KEY_ESC
    map.insert("esc".to_string(), 1);         // KEY_ESC
    map.insert("return".to_string(), 28);     // KEY_ENTER
    map.insert("enter".to_string(), 28);      // KEY_ENTER
    map.insert("tab".to_string(), 15);        // KEY_TAB
    map.insert("backspace".to_string(), 14);  // KEY_BACKSPACE
    map.insert("delete".to_string(), 111);    // KEY_DELETE
    map.insert("insert".to_string(), 110);    // KEY_INSERT
    map.insert("home".to_string(), 102);      // KEY_HOME
    map.insert("end".to_string(), 107);       // KEY_END
    map.insert("pageup".to_string(), 104);    // KEY_PAGEUP
    map.insert("pagedown".to_string(), 109);  // KEY_PAGEDOWN
    map.insert("space".to_string(), 57);      // KEY_SPACE
    map.insert("capslock".to_string(), 58);   // KEY_CAPSLOCK
    map.insert("numlock".to_string(), 69);    // KEY_NUMLOCK
    map.insert("scrolllock".to_string(), 70); // KEY_SCROLLLOCK
    map.insert("print".to_string(), 99);      // KEY_SYSRQ
    map.insert("printscreen".to_string(), 99);// KEY_SYSRQ
    map.insert("pause".to_string(), 119);     // KEY_PAUSE
    map.insert("menu".to_string(), 127);      // KEY_COMPOSE

    // Arrow keys
    map.insert("up".to_string(), 103);        // KEY_UP
    map.insert("down".to_string(), 108);      // KEY_DOWN
    map.insert("left".to_string(), 105);      // KEY_LEFT
    map.insert("right".to_string(), 106);     // KEY_RIGHT

    // Function keys
    for i in 1..=12 {
        map.insert(format!("f{}", i), 58 + i); // F1=59, F2=60, etc.
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
    let letter_codes = [30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17, 45, 21, 44];
    for (ch, code) in letters.chars().zip(letter_codes.iter()) {
        map.insert(ch.to_string(), *code);
    }

    map
}

/// Find the keycode for a character, and whether shift is needed
fn find_keycode_for_char(ch: char, keymap: &xkb::Keymap, _xkb_state: &xkb::State) -> Result<(u32, bool)> {
    let min_keycode: u32 = keymap.min_keycode().into();
    let max_keycode: u32 = keymap.max_keycode().into();

    // Try each keycode
    for keycode_raw in min_keycode..=max_keycode {
        let keycode = xkb::Keycode::new(keycode_raw);
        // Get number of layouts for this key
        let num_layouts = keymap.num_layouts_for_key(keycode);

        for layout in 0..num_layouts {
            let num_levels = keymap.num_levels_for_key(keycode, layout);

            for level in 0..num_levels {
                let syms = keymap.key_get_syms_by_level(keycode, layout, level);

                for sym in syms {
                    // Convert keysym to character
                    let sym_raw: u32 = (*sym).into();
                    if let Some(sym_char) = keysym_to_char(sym_raw) {
                        if sym_char == ch {
                            // Found it! Check if we need shift (level 1 typically means shift)
                            let need_shift = level == 1;
                            // Convert from xkb keycode (offset by 8) to evdev keycode
                            let evdev_keycode = keycode_raw - 8;
                            return Ok((evdev_keycode, need_shift));
                        }
                    }
                }
            }
        }
    }

    bail!("Could not find keycode for character: {:?}", ch)
}

/// Convert an XKB keysym to a character
fn keysym_to_char(keysym: u32) -> Option<char> {
    // For ASCII range, keysym often equals the character code
    if (0x20..=0x7e).contains(&keysym) {
        return char::from_u32(keysym);
    }

    // For Latin-1 supplement
    if (0xa0..=0xff).contains(&keysym) {
        return char::from_u32(keysym);
    }

    // Unicode keysyms (0x1000000 + unicode code point)
    if keysym >= 0x1000000 {
        return char::from_u32(keysym - 0x1000000);
    }

    // Special cases
    match keysym {
        0xff0d => Some('\n'), // Return
        0xff09 => Some('\t'), // Tab
        0x20 => Some(' '),    // Space
        _ => None,
    }
}

/// Get current timestamp in microseconds
fn get_timestamp() -> u64 {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let start = START.get_or_init(Instant::now);
    start.elapsed().as_micros() as u64
}

/// Connect to EI via the XDG RemoteDesktop portal
fn connect_via_portal() -> Result<UnixStream> {
    use ashpd::desktop::remote_desktop::{DeviceType, RemoteDesktop};
    use ashpd::desktop::PersistMode;
    use futures_executor::block_on;

    info!("Connecting via XDG RemoteDesktop portal...");

    block_on(async {
        let proxy = RemoteDesktop::new().await
            .context("Failed to create RemoteDesktop proxy")?;

        let session = proxy.create_session().await
            .context("Failed to create session")?;

        proxy.select_devices(&session, DeviceType::Keyboard.into(), None, PersistMode::DoNot).await
            .context("Failed to select devices")?;

        let _response = proxy.start(&session, None).await
            .context("Failed to start session")?;

        let fd = proxy.connect_to_eis(&session).await
            .context("Failed to connect to EIS")?;

        let stream = UnixStream::from(fd);
        stream.set_nonblocking(true)
            .context("Failed to set non-blocking")?;

        Ok(stream)
    })
}

/// Connect to EI via socket
fn connect_via_socket(socket_path: Option<&str>) -> Result<UnixStream> {
    let path = if let Some(path) = socket_path {
        std::path::PathBuf::from(path)
    } else if let Ok(socket) = std::env::var("LIBEI_SOCKET") {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .context("XDG_RUNTIME_DIR not set")?;
        std::path::Path::new(&runtime_dir).join(socket)
    } else {
        bail!("No socket path provided and LIBEI_SOCKET not set. Use -p for portal or -s for socket path.");
    };

    info!("Connecting to socket: {:?}", path);

    let stream = UnixStream::connect(&path)
        .with_context(|| format!("Failed to connect to socket: {:?}", path))?;

    stream.set_nonblocking(true)
        .context("Failed to set non-blocking")?;

    Ok(stream)
}

/// Context for typing operations
struct TypeContext {
    connection: reis::event::Connection,
    device: reis::event::Device,
    keyboard: ei::Keyboard,
    keymap: Option<xkb::Keymap>,
    xkb_state: Option<xkb::State>,
    key_to_keycode: HashMap<String, u32>,
    delay: Duration,
    held_modifiers: Vec<u32>,
    sequence: u32,
    xkb_config: XkbConfig,
}

impl TypeContext {
    fn new(
        connection: reis::event::Connection,
        device: reis::event::Device,
        keyboard: ei::Keyboard,
        delay: Duration,
        xkb_config: XkbConfig,
    ) -> Self {
        Self {
            connection,
            device,
            keyboard,
            keymap: None,
            xkb_state: None,
            key_to_keycode: build_key_to_keycode_map(),
            delay,
            held_modifiers: Vec::new(),
            sequence: 1,
            xkb_config,
        }
    }

    fn setup_keymap(&mut self) -> Result<()> {
        let xkb_context = xkb::Context::new(xkb::CONTEXT_NO_FLAGS);

        // First, try to use the keymap provided by the EI server
        if let Some(keymap_info) = self.device.keymap() {
            // We need to duplicate the fd since new_from_fd takes ownership
            use std::os::fd::FromRawFd;
            use std::os::fd::IntoRawFd;
            let fd_dup = rustix::io::dup(&keymap_info.fd)
                .context("Failed to duplicate keymap fd")?;
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
            .context("Failed to read keymap from fd")?
            .context("Failed to compile keymap")?;

            let state = xkb::State::new(&keymap);

            self.keymap = Some(keymap);
            self.xkb_state = Some(state);
            info!("Keymap loaded from EI server");
            return Ok(());
        }

        // Fallback: use XKB configuration from CLI/environment or system defaults
        let rules = self.xkb_config.rules.as_deref().unwrap_or("");
        let model = self.xkb_config.model.as_deref().unwrap_or("");
        let layout = self.xkb_config.layout.as_deref().unwrap_or("");
        let variant = self.xkb_config.variant.as_deref().unwrap_or("");
        let options = self.xkb_config.options.clone();

        if self.xkb_config.is_specified() {
            info!(
                "Loading keymap from configuration: layout={}, variant={}, model={}",
                if layout.is_empty() { "(default)" } else { layout },
                if variant.is_empty() { "(none)" } else { variant },
                if model.is_empty() { "(default)" } else { model }
            );
        } else {
            info!("Loading system default keymap");
        }

        let keymap = xkb::Keymap::new_from_names(
            &xkb_context,
            rules,
            model,
            layout,
            variant,
            options,
            xkb::KEYMAP_COMPILE_NO_FLAGS,
        )
        .context("Failed to load keymap from XKB configuration")?;

        let state = xkb::State::new(&keymap);

        self.keymap = Some(keymap);
        self.xkb_state = Some(state);
        Ok(())
    }

    fn start_emulating(&mut self) -> Result<()> {
        let serial = self.connection.serial();
        self.device.device().start_emulating(serial, self.sequence);
        self.sequence += 1;
        self.connection.flush()?;
        Ok(())
    }

    fn stop_emulating(&mut self) -> Result<()> {
        let serial = self.connection.serial();
        self.device.device().stop_emulating(serial);
        self.connection.flush()?;
        Ok(())
    }

    fn send_frame(&self) -> Result<()> {
        let serial = self.connection.serial();
        let timestamp = get_timestamp();
        self.device.device().frame(serial, timestamp);
        self.connection.flush()?;
        Ok(())
    }

    fn press_key(&self, keycode: u32) -> Result<()> {
        trace!("Pressing key: {}", keycode);
        self.keyboard.key(keycode, KeyState::Press);
        self.send_frame()?;
        Ok(())
    }

    fn release_key(&self, keycode: u32) -> Result<()> {
        trace!("Releasing key: {}", keycode);
        self.keyboard.key(keycode, KeyState::Released);
        self.send_frame()?;
        Ok(())
    }

    fn tap_key(&self, keycode: u32) -> Result<()> {
        self.press_key(keycode)?;
        if !self.delay.is_zero() {
            std::thread::sleep(self.delay);
        }
        self.release_key(keycode)?;
        if !self.delay.is_zero() {
            std::thread::sleep(self.delay);
        }
        Ok(())
    }

    fn type_char(&self, ch: char) -> Result<()> {
        trace!("Typing character: {:?}", ch);

        if let (Some(keymap), Some(xkb_state)) = (&self.keymap, &self.xkb_state) {
            // Use keymap to find the correct keycode
            let (keycode, need_shift) = find_keycode_for_char(ch, keymap, xkb_state)?;

            if need_shift {
                let shift_keycode = self.key_to_keycode.get("shift").copied().unwrap_or(42);
                self.press_key(shift_keycode)?;
            }

            self.tap_key(keycode)?;

            if need_shift {
                let shift_keycode = self.key_to_keycode.get("shift").copied().unwrap_or(42);
                self.release_key(shift_keycode)?;
            }
        } else {
            // Fallback: try to find keycode from our map
            let ch_lower = ch.to_ascii_lowercase();
            if let Some(&keycode) = self.key_to_keycode.get(&ch_lower.to_string()) {
                let need_shift = ch.is_ascii_uppercase();

                if need_shift {
                    let shift_keycode = self.key_to_keycode.get("shift").copied().unwrap_or(42);
                    self.press_key(shift_keycode)?;
                }

                self.tap_key(keycode)?;

                if need_shift {
                    let shift_keycode = self.key_to_keycode.get("shift").copied().unwrap_or(42);
                    self.release_key(shift_keycode)?;
                }
            } else {
                warn!("Could not find keycode for character: {:?}", ch);
            }
        }

        Ok(())
    }

    fn type_text(&self, text: &str) -> Result<()> {
        debug!("Typing text: {:?}", text);
        for ch in text.chars() {
            self.type_char(ch)?;
        }
        Ok(())
    }

    fn press_special_key(&self, key_name: &str) -> Result<()> {
        let keycode = self.key_to_keycode
            .get(&key_name.to_lowercase())
            .copied()
            .ok_or_else(|| anyhow!("Unknown key: {}", key_name))?;

        debug!("Pressing special key: {} (keycode {})", key_name, keycode);
        self.tap_key(keycode)
    }

    fn hold_modifier(&mut self, mod_name: &str) -> Result<()> {
        let keycode = self.key_to_keycode
            .get(&mod_name.to_lowercase())
            .copied()
            .ok_or_else(|| anyhow!("Unknown modifier: {}", mod_name))?;

        debug!("Holding modifier: {} (keycode {})", mod_name, keycode);
        self.press_key(keycode)?;
        self.held_modifiers.push(keycode);
        Ok(())
    }

    fn press_modifier(&self, mod_name: &str) -> Result<()> {
        let keycode = self.key_to_keycode
            .get(&mod_name.to_lowercase())
            .copied()
            .ok_or_else(|| anyhow!("Unknown modifier: {}", mod_name))?;

        debug!("Pressing modifier: {} (keycode {})", mod_name, keycode);
        self.tap_key(keycode)
    }

    fn release_held_modifiers(&mut self) -> Result<()> {
        for keycode in self.held_modifiers.drain(..).rev().collect::<Vec<_>>() {
            debug!("Releasing held modifier keycode {}", keycode);
            self.release_key(keycode)?;
        }
        Ok(())
    }

    fn execute_actions(&mut self, actions: &[Action]) -> Result<()> {
        info!("Executing {} actions", actions.len());

        for action in actions {
            match action {
                Action::Type(text) => {
                    self.type_text(text)?;
                }
                Action::Key(key_name) => {
                    self.press_special_key(key_name)?;
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
        self.release_held_modifiers()?;

        Ok(())
    }
}

fn run(args: Args) -> Result<()> {
    // Build the list of actions
    let mut actions = Vec::new();

    // Add held modifiers first
    for m in &args.modifiers {
        actions.push(Action::ModifierHold(m.clone()));
    }

    // Interleave text, keys, and pressed modifiers
    for text in &args.text {
        actions.push(Action::Type(text.clone()));
    }

    for key in &args.keys {
        actions.push(Action::Key(key.clone()));
    }

    for m in &args.press_modifiers {
        actions.push(Action::ModifierPress(m.clone()));
    }

    if actions.is_empty() {
        bail!("No text or keys to type. Use --help for usage.");
    }

    let delay = Duration::from_millis(args.delay);
    let xkb_config = XkbConfig::from_args_and_env(&args);

    // Connect to EI
    let stream = if args.portal {
        connect_via_portal()?
    } else {
        connect_via_socket(args.socket.as_deref())?
    };

    // Create EI context
    let context = ei::Context::new(stream)
        .context("Failed to create EI context")?;

    // Perform handshake
    info!("Performing handshake...");
    let (connection, mut event_iter) = context.handshake_blocking("eitype", ContextType::Sender)
        .context("Handshake failed")?;

    info!("Connected! Waiting for devices...");

    // Process events until we get a keyboard device
    let mut type_ctx: Option<TypeContext> = None;

    for event_result in &mut event_iter {
        let event = event_result.context("Error processing event")?;
        trace!("Received event: {:?}", event);

        match event {
            EiEvent::Disconnected(disconnected) => {
                let reason = disconnected.reason;
                let explanation = &disconnected.explanation;
                error!("Disconnected: {:?} - {}", reason, explanation);
                bail!("Disconnected from EI server");
            }

            EiEvent::SeatAdded(seat_added) => {
                let seat = &seat_added.seat;
                debug!("Seat added: {:?}", seat.name());
                seat.bind_capabilities(&[DeviceCapability::Keyboard]);
                connection.flush()?;
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

                    let mut ctx = TypeContext::new(
                        connection.clone(),
                        device,
                        keyboard,
                        delay,
                        xkb_config.clone(),
                    );

                    // Setup keymap (from EI server, CLI/env config, or system default)
                    ctx.setup_keymap()?;

                    type_ctx = Some(ctx);
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

    let Some(mut ctx) = type_ctx else {
        bail!("No keyboard device found");
    };

    // Start emulating and execute actions
    ctx.start_emulating()?;

    if let Err(e) = ctx.execute_actions(&actions) {
        error!("Error executing actions: {}", e);
        ctx.release_held_modifiers()?;
        ctx.stop_emulating()?;
        return Err(e);
    }

    ctx.stop_emulating()?;

    info!("Done");
    Ok(())
}

fn main() {
    let args = Args::parse();

    // Setup logging
    let log_level = match args.verbose {
        0 => log::LevelFilter::Warn,
        1 => log::LevelFilter::Info,
        2 => log::LevelFilter::Debug,
        _ => log::LevelFilter::Trace,
    };

    env_logger::Builder::new()
        .filter_level(log_level)
        .format_timestamp(None)
        .init();

    if let Err(e) = run(args) {
        error!("{:#}", e);
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_key_to_keycode_map_modifiers() {
        let map = build_key_to_keycode_map();

        // Test modifier keys
        assert_eq!(map.get("shift"), Some(&42));
        assert_eq!(map.get("lshift"), Some(&42));
        assert_eq!(map.get("rshift"), Some(&54));
        assert_eq!(map.get("ctrl"), Some(&29));
        assert_eq!(map.get("control"), Some(&29));
        assert_eq!(map.get("alt"), Some(&56));
        assert_eq!(map.get("super"), Some(&125));
        assert_eq!(map.get("meta"), Some(&125));
        assert_eq!(map.get("win"), Some(&125));
    }

    #[test]
    fn test_build_key_to_keycode_map_special_keys() {
        let map = build_key_to_keycode_map();

        // Test special keys
        assert_eq!(map.get("escape"), Some(&1));
        assert_eq!(map.get("esc"), Some(&1));
        assert_eq!(map.get("return"), Some(&28));
        assert_eq!(map.get("enter"), Some(&28));
        assert_eq!(map.get("tab"), Some(&15));
        assert_eq!(map.get("backspace"), Some(&14));
        assert_eq!(map.get("space"), Some(&57));
    }

    #[test]
    fn test_build_key_to_keycode_map_arrow_keys() {
        let map = build_key_to_keycode_map();

        // Test arrow keys
        assert_eq!(map.get("up"), Some(&103));
        assert_eq!(map.get("down"), Some(&108));
        assert_eq!(map.get("left"), Some(&105));
        assert_eq!(map.get("right"), Some(&106));
    }

    #[test]
    fn test_build_key_to_keycode_map_function_keys() {
        let map = build_key_to_keycode_map();

        // Test function keys (F1=59, F2=60, etc.)
        assert_eq!(map.get("f1"), Some(&59));
        assert_eq!(map.get("f2"), Some(&60));
        assert_eq!(map.get("f10"), Some(&68));
        assert_eq!(map.get("f12"), Some(&70));
    }

    #[test]
    fn test_build_key_to_keycode_map_letters() {
        let map = build_key_to_keycode_map();

        // Test some letter keys
        assert_eq!(map.get("a"), Some(&30));
        assert_eq!(map.get("b"), Some(&48));
        assert_eq!(map.get("z"), Some(&44));
    }

    #[test]
    fn test_build_key_to_keycode_map_numbers() {
        let map = build_key_to_keycode_map();

        // Test number keys
        assert_eq!(map.get("0"), Some(&11));
        assert_eq!(map.get("1"), Some(&2));
        assert_eq!(map.get("9"), Some(&10));
    }

    #[test]
    fn test_keysym_to_char_ascii() {
        // Test ASCII printable range
        assert_eq!(keysym_to_char(0x20), Some(' '));
        assert_eq!(keysym_to_char(0x41), Some('A'));
        assert_eq!(keysym_to_char(0x61), Some('a'));
        assert_eq!(keysym_to_char(0x7a), Some('z'));
        assert_eq!(keysym_to_char(0x30), Some('0'));
        assert_eq!(keysym_to_char(0x39), Some('9'));
    }

    #[test]
    fn test_keysym_to_char_special() {
        // Test special keysyms
        assert_eq!(keysym_to_char(0xff0d), Some('\n')); // Return
        assert_eq!(keysym_to_char(0xff09), Some('\t')); // Tab
    }

    #[test]
    fn test_keysym_to_char_unicode() {
        // Test Unicode keysyms (0x1000000 + unicode code point)
        assert_eq!(keysym_to_char(0x1000000 + 0x00e9), Some('é')); // é
        assert_eq!(keysym_to_char(0x1000000 + 0x00f1), Some('ñ')); // ñ
    }

    #[test]
    fn test_keysym_to_char_latin1_supplement() {
        // Test Latin-1 supplement range (0xa0-0xff)
        assert_eq!(keysym_to_char(0xa0), Some('\u{a0}')); // Non-breaking space
        assert_eq!(keysym_to_char(0xe9), Some('é'));
        assert_eq!(keysym_to_char(0xf1), Some('ñ'));
    }

    #[test]
    fn test_keysym_to_char_invalid() {
        // Test invalid/unmapped keysyms
        assert_eq!(keysym_to_char(0x00), None);
        assert_eq!(keysym_to_char(0x1f), None); // Below ASCII printable
        assert_eq!(keysym_to_char(0x7f), None); // DEL character
        assert_eq!(keysym_to_char(0xff00), None); // Unknown special keysym
    }

    #[test]
    fn test_get_timestamp_monotonic() {
        // Test that timestamps are monotonically increasing
        let t1 = get_timestamp();
        std::thread::sleep(std::time::Duration::from_millis(1));
        let t2 = get_timestamp();
        assert!(t2 >= t1, "Timestamps should be monotonically increasing");
    }

    #[test]
    fn test_cli_parsing_basic() {
        // Test basic CLI parsing
        let args = Args::try_parse_from(["eitype", "hello"]).unwrap();
        assert_eq!(args.text, vec!["hello"]);
        assert_eq!(args.delay, 0);
        assert!(!args.portal);
    }

    #[test]
    fn test_cli_parsing_multiple_text() {
        let args = Args::try_parse_from(["eitype", "hello", "world"]).unwrap();
        assert_eq!(args.text, vec!["hello", "world"]);
    }

    #[test]
    fn test_cli_parsing_delay() {
        let args = Args::try_parse_from(["eitype", "-d", "100", "hello"]).unwrap();
        assert_eq!(args.delay, 100);
    }

    #[test]
    fn test_cli_parsing_portal() {
        let args = Args::try_parse_from(["eitype", "-p", "hello"]).unwrap();
        assert!(args.portal);
    }

    #[test]
    fn test_cli_parsing_keys() {
        let args = Args::try_parse_from(["eitype", "-k", "return", "-k", "tab"]).unwrap();
        assert_eq!(args.keys, vec!["return", "tab"]);
    }

    #[test]
    fn test_cli_parsing_modifiers() {
        let args = Args::try_parse_from(["eitype", "-M", "ctrl", "-M", "shift", "c"]).unwrap();
        assert_eq!(args.modifiers, vec!["ctrl", "shift"]);
        assert_eq!(args.text, vec!["c"]);
    }

    #[test]
    fn test_cli_parsing_socket() {
        let args = Args::try_parse_from(["eitype", "-s", "/tmp/eis-0", "hello"]).unwrap();
        assert_eq!(args.socket, Some("/tmp/eis-0".to_string()));
    }

    #[test]
    fn test_cli_parsing_verbose() {
        let args = Args::try_parse_from(["eitype", "-v", "hello"]).unwrap();
        assert_eq!(args.verbose, 1);

        let args = Args::try_parse_from(["eitype", "-vv", "hello"]).unwrap();
        assert_eq!(args.verbose, 2);

        let args = Args::try_parse_from(["eitype", "-vvv", "hello"]).unwrap();
        assert_eq!(args.verbose, 3);
    }

    #[test]
    fn test_action_enum() {
        // Test Action enum variants
        let type_action = Action::Type("hello".to_string());
        let key_action = Action::Key("return".to_string());
        let mod_hold = Action::ModifierHold("ctrl".to_string());
        let mod_press = Action::ModifierPress("shift".to_string());

        // Just verify they can be created and matched
        match type_action {
            Action::Type(s) => assert_eq!(s, "hello"),
            _ => panic!("Wrong variant"),
        }
        match key_action {
            Action::Key(s) => assert_eq!(s, "return"),
            _ => panic!("Wrong variant"),
        }
        match mod_hold {
            Action::ModifierHold(s) => assert_eq!(s, "ctrl"),
            _ => panic!("Wrong variant"),
        }
        match mod_press {
            Action::ModifierPress(s) => assert_eq!(s, "shift"),
            _ => panic!("Wrong variant"),
        }
    }

    #[test]
    fn test_xkb_config_default() {
        let config = XkbConfig::default();
        assert!(config.rules.is_none());
        assert!(config.model.is_none());
        assert!(config.layout.is_none());
        assert!(config.variant.is_none());
        assert!(config.options.is_none());
        assert!(!config.is_specified());
    }

    #[test]
    fn test_xkb_config_is_specified() {
        let mut config = XkbConfig::default();
        assert!(!config.is_specified());

        config.layout = Some("us".to_string());
        assert!(config.is_specified());

        config.layout = None;
        config.variant = Some("dvorak".to_string());
        assert!(config.is_specified());
    }

    #[test]
    fn test_cli_parsing_layout() {
        let args = Args::try_parse_from(["eitype", "-l", "de", "hello"]).unwrap();
        assert_eq!(args.layout, Some("de".to_string()));
    }

    #[test]
    fn test_cli_parsing_layout_long() {
        let args = Args::try_parse_from(["eitype", "--layout", "fr", "hello"]).unwrap();
        assert_eq!(args.layout, Some("fr".to_string()));
    }

    #[test]
    fn test_cli_parsing_variant() {
        let args = Args::try_parse_from(["eitype", "--variant", "dvorak", "hello"]).unwrap();
        assert_eq!(args.variant, Some("dvorak".to_string()));
    }

    #[test]
    fn test_cli_parsing_model() {
        let args = Args::try_parse_from(["eitype", "--model", "pc105", "hello"]).unwrap();
        assert_eq!(args.model, Some("pc105".to_string()));
    }

    #[test]
    fn test_cli_parsing_options() {
        let args = Args::try_parse_from(["eitype", "--options", "ctrl:nocaps", "hello"]).unwrap();
        assert_eq!(args.options, Some("ctrl:nocaps".to_string()));
    }

    #[test]
    fn test_cli_parsing_full_xkb_config() {
        let args = Args::try_parse_from([
            "eitype",
            "-l", "us",
            "--variant", "dvorak",
            "--model", "pc104",
            "--options", "ctrl:nocaps",
            "hello"
        ]).unwrap();
        assert_eq!(args.layout, Some("us".to_string()));
        assert_eq!(args.variant, Some("dvorak".to_string()));
        assert_eq!(args.model, Some("pc104".to_string()));
        assert_eq!(args.options, Some("ctrl:nocaps".to_string()));
    }

    #[test]
    fn test_xkb_config_from_args() {
        let args = Args::try_parse_from([
            "eitype",
            "-l", "de",
            "--variant", "nodeadkeys",
            "hello"
        ]).unwrap();

        let config = XkbConfig::from_args_and_env(&args);
        assert_eq!(config.layout, Some("de".to_string()));
        assert_eq!(config.variant, Some("nodeadkeys".to_string()));
        assert!(config.is_specified());
    }
}

