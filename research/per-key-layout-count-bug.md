# Bug: `find_keycode_for_char` Skips Keys Missing the Requested Layout Index

## Summary

When `layout_index > 0`, `find_keycode_for_char()` silently skips keys that don't define that layout index, causing "Character not found in keymap" errors for common characters like space.

## How to Reproduce

1. Have two keyboard layouts configured: Programmer Dvorak (`us+dvp`, index 0) and US QWERTY (`us`, index 1)
2. Be on Dvorak (your active layout, index 0)
3. Override to QWERTY: `cargo run -- --layout-index 1 "hello world"`
4. Error: `Character not found in keymap: ` (space)

Even though the *keymap* has layouts globally, individual keys may only define entries for a subset of layouts.

## Root Cause

In `src/lib.rs`, `find_keycode_for_char()` (around line 250):

```rust
for keycode_raw in min_keycode..=max_keycode {
    let keycode = xkb::Keycode::new(keycode_raw);
    let num_layouts = keymap.num_layouts_for_key(keycode);  // per-KEY count

    if layout_index < num_layouts {  // <-- skips if key doesn't have this layout
        // ... search for character in this key's levels
    }
}
```

`keymap.num_layouts_for_key(keycode)` returns how many layouts a *specific key* defines — NOT the global keymap layout count. Some keys (like space, modifiers, function keys) may only define 1 layout. When `layout_index == 1`, the check `1 < 1` is false, so the key is skipped entirely.

This means layout-independent characters (space, enter, tab, digits on some keymaps) become unfindable at non-zero layout indices.

## Observed Behavior

```
$ cargo run -- --layout-index 1 "hello world"
[ERROR eitype] Error executing actions: Character not found in keymap:
[ERROR eitype] Character not found in keymap:
```

The space character (keysym `0x20`) exists at keycode 65 (XKB) / 57 (evdev) but `num_layouts_for_key(65)` returns 1, so it's skipped when `layout_index == 1`.

## Proposed Fix

If a key doesn't have the requested layout index, fall back to layout 0 for that key. Characters like space are layout-independent — they produce the same keysym regardless of layout.

```rust
for keycode_raw in min_keycode..=max_keycode {
    let keycode = xkb::Keycode::new(keycode_raw);
    let num_layouts = keymap.num_layouts_for_key(keycode);

    // Use requested layout if the key defines it, otherwise fall back to layout 0
    let effective_layout = if layout_index < num_layouts {
        layout_index
    } else if num_layouts > 0 {
        0
    } else {
        continue;
    };

    let num_levels = keymap.num_levels_for_key(keycode, effective_layout);
    // ... rest of lookup using effective_layout
}
```

### Considerations

- **Correctness**: This is safe because if a key only defines layout 0, it means the key produces the same output regardless of which layout group is active. The XKB spec says keys inherit from the base layout when they don't define a specific group.
- **Priority**: The lookup should still prefer keys that explicitly define the requested layout. A two-pass approach may be cleaner:
  1. First pass: search only keys that have `layout_index` defined (exact match)
  2. Second pass: search keys at layout 0 as fallback (for layout-independent keys)

  This prevents a layout-0 key from shadowing a different key at the requested layout.

## Affected Code

| What | File | Line (approximate) |
|------|------|--------------------|
| `find_keycode_for_char()` | `src/lib.rs` | ~250-283 |
| `layout_index` field | `src/lib.rs` | `EiType.layout_index` |
| Called from `type_char()` | `src/lib.rs` | ~802 |

## Branch

This should be fixed on a new branch off `main` (after merging the `auto-detect-layout-group` PR).
