# eitype

A wtype-like CLI tool for typing text using Emulated Input (EI) protocol on Wayland.

## Features

- Type text using the EI (Emulated Input) protocol
- Support for special keys (enter, tab, escape, arrows, function keys, etc.)
- Support for modifier keys (shift, ctrl, alt, super)
- XDG RemoteDesktop portal support
- Direct socket connection support
- Configurable delay between key events
- Keyboard layout configuration via CLI or environment variables

## Installation

```bash
cargo install --path .
```

### Dependencies

- libxkbcommon (libxkbcommon-dev on Debian/Ubuntu)
- Rust 1.70+

## Usage

```bash
# Type text (requires EI server or portal)
eitype "Hello, World!"

# Type text using the XDG RemoteDesktop portal
eitype -p "Hello, World!"

# Type with delay between keys (10ms)
eitype -d 10 "Slow typing..."

# Press special keys
eitype -k return
eitype -k tab
eitype -k escape

# Hold modifier while typing
eitype -M ctrl c  # Ctrl+C

# Press and release a modifier
eitype -P shift

# Multiple texts
eitype "First line" -k return "Second line"

# Verbose output
eitype -v "Debug mode"
eitype -vv "More debug"
```

## Connection Methods

### XDG RemoteDesktop Portal (Recommended)

Use the `-p` flag to connect via the XDG RemoteDesktop portal. This is the recommended method for desktop environments that support it (GNOME, KDE, etc.).

```bash
eitype -p "Hello"
```

### Direct Socket

Use the `-s` flag to specify a socket path, or set the `LIBEI_SOCKET` environment variable:

```bash
eitype -s /path/to/ei/socket "Hello"
# or
export LIBEI_SOCKET=eis-0
eitype "Hello"
```

## Special Keys

Supported special key names (case-insensitive):
- `escape`, `esc`
- `return`, `enter`
- `tab`
- `backspace`
- `delete`
- `insert`
- `home`, `end`
- `pageup`, `pagedown`
- `up`, `down`, `left`, `right`
- `f1` through `f12`
- `space`
- `capslock`, `numlock`, `scrolllock`
- `print`, `printscreen`
- `pause`, `menu`

## Modifier Keys

Supported modifier names (case-insensitive):
- `shift`, `lshift`, `rshift`
- `ctrl`, `control`, `lctrl`, `rctrl`
- `alt`, `lalt`, `ralt`, `altgr`
- `super`, `meta`, `win`, `lsuper`, `rsuper`

## Keyboard Layout

eitype uses XKB for keyboard layout handling. The keymap is determined in the following order:

1. **EI server keymap** - If the EI server provides a keymap, it is used automatically
2. **CLI/environment configuration** - If no server keymap, uses specified layout
3. **System default** - Falls back to the system's default XKB configuration

### CLI Options

```bash
# Use German keyboard layout
eitype -l de "Hallo Welt"

# Use US Dvorak layout
eitype -l us --variant dvorak "Hello"

# Full XKB configuration
eitype -l us --variant dvorak --model pc104 --options "ctrl:nocaps" "Hello"
```

### Environment Variables

You can also set keyboard layout via environment variables (CLI options take precedence):

- `XKB_DEFAULT_LAYOUT` - Keyboard layout (e.g., "us", "de", "fr")
- `XKB_DEFAULT_VARIANT` - Layout variant (e.g., "dvorak", "colemak", "nodeadkeys")
- `XKB_DEFAULT_MODEL` - Keyboard model (e.g., "pc104", "pc105")
- `XKB_DEFAULT_OPTIONS` - XKB options (e.g., "ctrl:nocaps")
- `XKB_DEFAULT_RULES` - XKB rules file

```bash
# Set German layout via environment
export XKB_DEFAULT_LAYOUT=de
eitype "Hallo"

# Override with CLI
XKB_DEFAULT_LAYOUT=de eitype -l fr "Bonjour"  # Uses French layout
```

## License

MIT
