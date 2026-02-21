//! eitype CLI - A wtype-like tool for typing text using Emulated Input (EI) on Wayland
//!
//! This is the command-line interface for the eitype library.

use anyhow::{bail, Context, Result};
use clap::Parser;
use eitype::{Action, EiType, EiTypeConfig};
use log::{error, info, warn};
use std::fs;
use std::path::PathBuf;

// ============================================================================
// Token Storage (CLI-only, not in library)
// ============================================================================

fn get_token_path() -> PathBuf {
    let cache_dir = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".cache")
        });
    cache_dir.join("eitype").join("restore_token")
}

fn load_restore_token() -> Option<String> {
    let path = get_token_path();
    match fs::read_to_string(&path) {
        Ok(token) => {
            let token = token.trim().to_string();
            if !token.is_empty() {
                info!("Loaded restore token from {:?}", path);
                Some(token)
            } else {
                None
            }
        }
        Err(_) => None,
    }
}

fn save_restore_token(token: &str) -> Result<()> {
    let path = get_token_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create token directory: {:?}", parent))?;
    }
    fs::write(&path, token)
        .with_context(|| format!("Failed to save restore token to {:?}", path))?;
    info!("Saved restore token to {:?}", path);
    Ok(())
}

fn clear_restore_token() -> Result<()> {
    let path = get_token_path();
    if path.exists() {
        fs::remove_file(&path)
            .with_context(|| format!("Failed to remove token file: {:?}", path))?;
        info!("Cleared restore token from {:?}", path);
    }
    Ok(())
}

// ============================================================================
// CLI Arguments
// ============================================================================

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

    /// Socket path for direct connection (defaults to LIBEI_SOCKET env var).
    /// If not specified, uses XDG RemoteDesktop portal.
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

    /// XKB layout index to use when multiple layouts are available.
    /// If not specified, auto-detects from the compositor (GNOME, KDE, Sway).
    #[arg(long, value_name = "INDEX")]
    layout_index: Option<u32>,

    /// Verbose output
    #[arg(short = 'v', long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Clear saved portal session token and force new authorization dialog
    #[arg(long)]
    reset_token: bool,
}

impl Args {
    /// Convert CLI args to EiTypeConfig
    fn to_config(&self) -> EiTypeConfig {
        EiTypeConfig {
            layout: self
                .layout
                .clone()
                .or_else(|| std::env::var("XKB_DEFAULT_LAYOUT").ok()),
            variant: self
                .variant
                .clone()
                .or_else(|| std::env::var("XKB_DEFAULT_VARIANT").ok()),
            model: self
                .model
                .clone()
                .or_else(|| std::env::var("XKB_DEFAULT_MODEL").ok()),
            options: self
                .options
                .clone()
                .or_else(|| std::env::var("XKB_DEFAULT_OPTIONS").ok()),
            layout_index: self.layout_index,
            delay_ms: self.delay,
        }
    }

    /// Build list of actions from CLI args
    fn to_actions(&self) -> Vec<Action> {
        let mut actions = Vec::new();

        // Add held modifiers first
        for m in &self.modifiers {
            actions.push(Action::ModifierHold(m.clone()));
        }

        // Add text
        for text in &self.text {
            actions.push(Action::Type(text.clone()));
        }

        // Add keys
        for key in &self.keys {
            actions.push(Action::Key(key.clone()));
        }

        // Add pressed modifiers
        for m in &self.press_modifiers {
            actions.push(Action::ModifierPress(m.clone()));
        }

        actions
    }
}

/// Get socket path from CLI arg or LIBEI_SOCKET environment variable.
fn get_socket_path(socket_arg: Option<&str>) -> Option<PathBuf> {
    if let Some(path) = socket_arg {
        return Some(PathBuf::from(path));
    }
    if let Ok(socket) = std::env::var("LIBEI_SOCKET") {
        if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
            return Some(std::path::Path::new(&runtime_dir).join(socket));
        }
    }
    None
}

// ============================================================================
// Main
// ============================================================================

fn run(args: Args) -> Result<()> {
    let actions = args.to_actions();

    if actions.is_empty() {
        bail!("No text or keys to type. Use --help for usage.");
    }

    let config = args.to_config();

    // Handle --reset-token flag
    if args.reset_token {
        clear_restore_token()?;
    }

    // Connect to EI
    let mut eitype = if let Some(socket_path) = get_socket_path(args.socket.as_deref()) {
        // Socket path specified via -s or LIBEI_SOCKET
        EiType::connect_socket(&socket_path, config)?
    } else {
        // Default to portal (with session persistence)
        let saved_token = if args.reset_token {
            None
        } else {
            load_restore_token()
        };

        let (eitype, new_token) =
            EiType::connect_portal_with_token(config, saved_token.as_deref())?;

        // Save new token for future runs
        if let Some(token) = new_token {
            if let Err(e) = save_restore_token(&token) {
                warn!("Failed to save restore token: {}", e);
            }
        }

        eitype
    };

    // Execute actions
    if let Err(e) = eitype.execute_actions(&actions) {
        error!("Error executing actions: {}", e);
        return Err(e.into());
    }

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
        .filter_level(log::LevelFilter::Warn)
        .filter_module("eitype", log_level)
        .format_timestamp(None)
        .init();

    if let Err(e) = run(args) {
        error!("{:#}", e);
        std::process::exit(1);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parsing_basic() {
        let args = Args::try_parse_from(["eitype", "hello"]).unwrap();
        assert_eq!(args.text, vec!["hello"]);
        assert_eq!(args.delay, 0);
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
    }

    #[test]
    fn test_cli_parsing_layout() {
        let args = Args::try_parse_from(["eitype", "-l", "de", "hello"]).unwrap();
        assert_eq!(args.layout, Some("de".to_string()));
    }

    #[test]
    fn test_cli_parsing_full_xkb_config() {
        let args = Args::try_parse_from([
            "eitype",
            "-l",
            "us",
            "--variant",
            "dvorak",
            "--model",
            "pc104",
            "--options",
            "ctrl:nocaps",
            "hello",
        ])
        .unwrap();
        assert_eq!(args.layout, Some("us".to_string()));
        assert_eq!(args.variant, Some("dvorak".to_string()));
        assert_eq!(args.model, Some("pc104".to_string()));
        assert_eq!(args.options, Some("ctrl:nocaps".to_string()));
    }

    #[test]
    fn test_to_config() {
        let args = Args::try_parse_from([
            "eitype",
            "-l",
            "de",
            "--variant",
            "nodeadkeys",
            "-d",
            "50",
            "hello",
        ])
        .unwrap();

        let config = args.to_config();
        assert_eq!(config.layout, Some("de".to_string()));
        assert_eq!(config.variant, Some("nodeadkeys".to_string()));
        assert_eq!(config.delay_ms, 50);
    }

    #[test]
    fn test_to_actions() {
        let args = Args::try_parse_from([
            "eitype", "-M", "ctrl", "hello", "-k", "return", "-P", "shift",
        ])
        .unwrap();

        let actions = args.to_actions();
        assert_eq!(actions.len(), 4);

        // ModifierHold comes first
        assert!(matches!(&actions[0], Action::ModifierHold(m) if m == "ctrl"));
        // Then text
        assert!(matches!(&actions[1], Action::Type(t) if t == "hello"));
        // Then keys
        assert!(matches!(&actions[2], Action::Key(k) if k == "return"));
        // Then pressed modifiers
        assert!(matches!(&actions[3], Action::ModifierPress(m) if m == "shift"));
    }
}
