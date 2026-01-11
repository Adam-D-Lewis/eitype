# eitype

A wtype-like CLI tool for typing text using Emulated Input (EI) protocol on Wayland.

## Features

- Type text using the EI (Emulated Input) protocol
- Support for special keys (enter, tab, escape, arrows, function keys, etc.)
- Support for modifier keys (shift, ctrl, alt, super)
- XDG RemoteDesktop portal support
- Direct socket connection support
- Configurable delay between key events

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

## License

MIT
