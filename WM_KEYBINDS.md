# Window Manager Keybind Configuration

The NAV layer sends standard Super-key combos for workspace and monitor navigation. Configure your window manager to match the table below and the bindings will work transparently.

## What the keyboard sends

| NAV key | Keys sent | Action |
|---------|-----------|--------|
| Tab | `Super+U` | Focus workspace below / next |
| Q | `Super+I` | Focus workspace above / prev |
| Esc | `Super+Shift+H` | Focus monitor left |
| A | `Super+Shift+J` | Focus monitor down |
| S (tap; hold = Alt) | `Super+Shift+K` | Focus monitor up |
| D (tap; hold = Ctrl) | `Super+Shift+L` | Focus monitor right |
| `\` | `Super+Ctrl+U` | Move column/window to workspace below |
| Z | `Super+Ctrl+I` | Move column/window to workspace above |
| X | `Super+Ctrl+Shift+H` | Move column/window to monitor left |
| C | `Super+Ctrl+Shift+L` | Move column/window to monitor right |
| N | `Super+1` | Go to workspace 1 (first) |
| M | `Super+U` | Go to workspace next |
| `,` | `Super+I` | Go to workspace prev |
| `.` | `Super+9` | Go to workspace last |
| `/` | `Super+Shift+H` | Focus monitor left |
| RET | `Super+Shift+L` | Focus monitor right |

---

## niri

Add the following entries inside your `binds {}` block in `~/.config/niri/config.kdl`:

```kdl
// Workspace navigation
Mod+U { focus-workspace-down; }
Mod+I { focus-workspace-up; }
Mod+1 { focus-workspace 1; }
Mod+2 { focus-workspace 2; }
Mod+3 { focus-workspace 3; }
Mod+4 { focus-workspace 4; }
Mod+5 { focus-workspace 5; }
Mod+6 { focus-workspace 6; }
Mod+7 { focus-workspace 7; }
Mod+8 { focus-workspace 8; }
Mod+9 { focus-workspace 9; }

// Move column to workspace
Mod+Ctrl+U { move-column-to-workspace-down; }
Mod+Ctrl+I { move-column-to-workspace-up; }

// Monitor focus
Mod+Shift+H { focus-monitor-left; }
Mod+Shift+J { focus-monitor-down; }
Mod+Shift+K { focus-monitor-up; }
Mod+Shift+L { focus-monitor-right; }

// Move column to monitor
Mod+Shift+Ctrl+H { move-column-to-monitor-left; }
Mod+Shift+Ctrl+L { move-column-to-monitor-right; }
```

> **Note:** `Mod` in niri equals `Super` (Win key) when running on a TTY, or `Alt` when running inside a winit window. The keyboard sends `LGUI` (Left Super), so TTY mode is the intended target.

---

## GNOME

GNOME uses Super as its primary modifier. Set shortcuts via **Settings → Keyboard → Keyboard Shortcuts**, or apply all at once with `gsettings`:

```bash
# Workspace focus
gsettings set org.gnome.desktop.wm.keybindings switch-to-workspace-down "['<Super>u']"
gsettings set org.gnome.desktop.wm.keybindings switch-to-workspace-up   "['<Super>i']"
gsettings set org.gnome.desktop.wm.keybindings switch-to-workspace-1    "['<Super>1']"
gsettings set org.gnome.desktop.wm.keybindings switch-to-workspace-last "['<Super>9']"

# Move window to workspace
gsettings set org.gnome.desktop.wm.keybindings move-to-workspace-down "['<Super><Control>u']"
gsettings set org.gnome.desktop.wm.keybindings move-to-workspace-up   "['<Super><Control>i']"

# Workspace 1–9 (optional — remove GNOME's default Super+1-9 app shortcuts first)
gsettings set org.gnome.desktop.wm.keybindings switch-to-workspace-2 "['<Super>2']"
gsettings set org.gnome.desktop.wm.keybindings switch-to-workspace-3 "['<Super>3']"
# ... repeat for 4–8
```

**Monitor navigation** — GNOME does not have built-in keyboard shortcuts for focusing monitors. Options:

- Install the [**Auto Move Windows**](https://extensions.gnome.org/extension/16/auto-move-windows/) or [**Workspace Matrix**](https://extensions.gnome.org/extension/1485/workspace-matrix/) extensions, which add monitor-aware workspace navigation.
- Use `gdbus` calls in custom keyboard shortcuts:
  ```bash
  # Example custom shortcut for "move window to right monitor"
  # Settings → Keyboard → Custom Shortcuts → add command:
  gdbus call --session --dest org.gnome.Shell \
    --object-path /org/gnome/Shell \
    --method org.gnome.Shell.Eval \
    'global.display.get_focus_window().move_to_monitor(1)'
  ```
- The `Super+Shift+H/J/K/L` keys sent by the keyboard can be bound to these custom shortcuts.

**Disable conflicting defaults** (GNOME uses Super+1-9 for dash-to-dock app launching):
```bash
for i in $(seq 1 9); do
  gsettings set org.gnome.shell.keybindings switch-to-application-$i "[]"
done
```

---

## COSMIC (System76)

COSMIC Desktop uses Super similarly to niri. Configure via **Settings → Keyboard → Keyboard Shortcuts → Custom Shortcuts** or the keybindings config file at `~/.config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/custom`.

```
# Workspace navigation
Super+U  → Switch to next workspace
Super+I  → Switch to previous workspace
Super+1  → Switch to workspace 1
Super+9  → Switch to workspace 9

# Move window to workspace
Super+Ctrl+U  → Move window to next workspace
Super+Ctrl+I  → Move window to previous workspace

# Monitor focus
Super+Shift+H  → Focus left output
Super+Shift+J  → Focus lower output
Super+Shift+K  → Focus upper output
Super+Shift+L  → Focus right output

# Move window to monitor
Super+Ctrl+Shift+H  → Move window to left output
Super+Ctrl+Shift+L  → Move window to right output
```

> **Note:** COSMIC's keybinding UI is still evolving (as of 2026). If the settings panel does not expose all actions, edit the config file directly. Refer to the [COSMIC documentation](https://github.com/pop-os/cosmic-epoch) for the current action names.

---

## Tips

- **Super = LG (Left GUI) on the keyboard** — all these combos are sent with the left Super/Win key.
- The `/edit-keymap` skill has the full NAV layout if you need to remap any of these to different keys.
- If you add a new WM or change these bindings, update this file so future sessions stay in sync.
