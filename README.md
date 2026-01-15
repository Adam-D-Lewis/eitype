# eitype

A library and CLI tool for typing text using the Emulated Input (EI) protocol on Wayland.

## Features

- Type text using the EI (Emulated Input) protocol
- Support for special keys (enter, tab, escape, arrows, function keys, etc.)
- Support for modifier keys (shift, ctrl, alt, super)
- XDG RemoteDesktop portal support with session persistence
- Direct socket connection support
- Configurable delay between key events
- Keyboard layout configuration via CLI or environment variables
- **Python bindings** for use in Python applications
- **Rust library** for integration into other Rust projects

## Installation

### CLI

```bash
cargo install --path .
```

### Python

```bash
# Using pixi (recommended)
pixi run build

# Or using maturin directly
pip install maturin
maturin develop --features python
```

## Usage

```bash
# Type text (uses XDG RemoteDesktop portal by default)
eitype "Hello, World!"

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

### XDG RemoteDesktop Portal (Default)

By default, eitype connects via the XDG RemoteDesktop portal. This works with desktop environments that support it (GNOME, KDE, etc.).

```bash
eitype "Hello"
```

#### Session Persistence

eitype automatically saves a session token to avoid the authorization dialog on subsequent runs. The token is stored at `~/.cache/eitype/restore_token`.

- **First run**: Shows the authorization dialog, saves token for future use
- **Subsequent runs**: Uses saved token, no dialog needed
- **Token expiration**: If the token becomes invalid, a new dialog will appear

To force a new authorization dialog (clear the saved token):

```bash
eitype --reset-token "Hello"
```

### Direct Socket

Use the `-s` flag to specify a socket path, or set the `LIBEI_SOCKET` environment variable to bypass the portal:

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

# Select a specific layout index when multiple layouts are available
eitype --layout-index 1 "Hello"
```

### Multi-Layout Keymaps

When the EI server provides a keymap with multiple layouts (e.g., Dvorak + QWERTY), eitype uses layout index 0 by default. This is typically correct since layout 0 is the first/active layout.

If you need to use a different layout, specify it with `--layout-index`:

```bash
# Use the second layout (index 1)
eitype --layout-index 1 "Hello"
```

Use `-vv` to see all available layouts in the keymap.

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

## Python Usage

```python
from eitype import EiType, EiTypeConfig

# Simple connection via portal
typer = EiType.connect_portal()
typer.type_text("Hello from Python!")
typer.press_key("Return")

# With custom configuration
config = EiTypeConfig(layout="de", delay_ms=10)
typer = EiType.connect_portal(config)
typer.type_text("Hallo Welt!")

# Modifier keys
typer.hold_modifier("ctrl")
typer.press_key("c")
typer.release_modifiers()
```

### Token Persistence (for long-running apps)

For applications that run continuously (like voice typing tools), you can save and reuse the portal authorization token:

```python
from eitype import EiType

# First run - will show authorization dialog
typer, token = EiType.connect_portal_with_token()
if token:
    save_to_config(token)  # Save for next time

# Subsequent runs - no dialog needed
saved_token = load_from_config()
typer, _ = EiType.connect_portal_with_token(saved_token)
typer.type_text("No dialog this time!")
```

## Rust Library Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
eitype = { path = "../eitype" }  # or from crates.io when published
```

```rust
use eitype::{EiType, EiTypeConfig, EiTypeError};

fn main() -> Result<(), EiTypeError> {
    // Connect via portal
    let mut typer = EiType::connect_portal(EiTypeConfig::default())?;

    // Type text
    typer.type_text("Hello from Rust!")?;
    typer.press_key("Return")?;

    // With modifiers
    typer.hold_modifier("ctrl")?;
    typer.press_key("c")?;
    typer.release_modifiers()?;

    Ok(())
}
```

## Building from Source

### System Dependencies

Before building eitype, install the required system libraries:

| Distribution | Command |
|--------------|---------|
| Debian/Ubuntu | `sudo apt install libxkbcommon-dev` |
| Fedora/RHEL | `sudo dnf install libxkbcommon-devel` |
| Arch Linux | `sudo pacman -S libxkbcommon` |
| openSUSE | `sudo zypper install libxkbcommon-devel` |

### Build Requirements

- Rust 1.70+
- Python 3.10+ (only for Python bindings)

## License

Apache 2.0
