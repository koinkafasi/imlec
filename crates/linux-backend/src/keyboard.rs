use evdev::{Device, EventSummary, KeyCode, RelativeAxisCode};
use pc_core::KeyClass;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone, Copy)]
pub enum InputSignal {
    Key(KeyClass),
    /// Raw relative pointer motion, used only by the fallback pointer tracker.
    Motion {
        dx: f32,
        dy: f32,
    },
}

const BTN_MISC: u16 = 0x100;

fn classify(code: KeyCode) -> KeyClass {
    // Mouse and gamepad buttons share EV_KEY with the keyboard; they start at BTN_MISC.
    if code.code() >= BTN_MISC {
        return KeyClass::Ignore;
    }
    if code == KeyCode::KEY_BACKSPACE || code == KeyCode::KEY_DELETE {
        return KeyClass::Delete;
    }
    const MODIFIERS: [KeyCode; 12] = [
        KeyCode::KEY_LEFTSHIFT,
        KeyCode::KEY_RIGHTSHIFT,
        KeyCode::KEY_LEFTCTRL,
        KeyCode::KEY_RIGHTCTRL,
        KeyCode::KEY_LEFTALT,
        KeyCode::KEY_RIGHTALT,
        KeyCode::KEY_LEFTMETA,
        KeyCode::KEY_RIGHTMETA,
        KeyCode::KEY_CAPSLOCK,
        KeyCode::KEY_NUMLOCK,
        KeyCode::KEY_SCROLLLOCK,
        KeyCode::KEY_COMPOSE,
    ];
    if MODIFIERS.contains(&code) {
        return KeyClass::Ignore;
    }
    KeyClass::Text
}

fn is_keyboard(dev: &Device) -> bool {
    dev.supported_keys().is_some_and(|keys| {
        keys.contains(KeyCode::KEY_A)
            && keys.contains(KeyCode::KEY_Z)
            && keys.contains(KeyCode::KEY_SPACE)
    })
}

fn is_pointer(dev: &Device) -> bool {
    dev.supported_relative_axes()
        .is_some_and(|axes| axes.contains(RelativeAxisCode::REL_X))
}

/// Starts reading every keyboard and pointer under /dev/input and rescans for
/// hotplugged devices. Requires membership of the `input` group; devices that
/// cannot be opened are skipped.
pub fn spawn<F>(sink: F) -> std::io::Result<()>
where
    F: Fn(InputSignal) + Send + Clone + 'static,
{
    let active: Arc<Mutex<HashSet<PathBuf>>> = Arc::new(Mutex::new(HashSet::new()));
    let scan_active = Arc::clone(&active);

    std::thread::Builder::new()
        .name("evdev-scan".into())
        .spawn(move || loop {
            scan(&scan_active, &sink);
            std::thread::sleep(Duration::from_secs(3));
        })?;
    Ok(())
}

fn scan<F>(active: &Arc<Mutex<HashSet<PathBuf>>>, sink: &F)
where
    F: Fn(InputSignal) + Send + Clone + 'static,
{
    for (path, device) in evdev::enumerate() {
        {
            let guard = active.lock().unwrap();
            if guard.contains(&path) {
                continue;
            }
        }
        let keyboard = is_keyboard(&device);
        let pointer = is_pointer(&device);
        if !keyboard && !pointer {
            continue;
        }

        active.lock().unwrap().insert(path.clone());
        let sink = sink.clone();
        let active = Arc::clone(active);
        let name = device.name().unwrap_or("unknown").to_string();
        let thread = std::thread::Builder::new()
            .name(format!("evdev-{}", path.display()))
            .spawn(move || {
                log::info!("reading {} ({})", path.display(), name);
                read_device(device, keyboard, pointer, &sink);
                log::info!("stopped reading {}", path.display());
                active.lock().unwrap().remove(&path);
            });
        if let Err(err) = thread {
            log::warn!("failed to spawn reader thread: {err}");
        }
    }
}

fn read_device<F>(mut device: Device, keyboard: bool, pointer: bool, sink: &F)
where
    F: Fn(InputSignal),
{
    loop {
        let events = match device.fetch_events() {
            Ok(events) => events,
            // Device unplugged or read error: let the scanner pick it up again later.
            Err(err) => {
                log::debug!("device read ended: {err}");
                return;
            }
        };
        for event in events {
            match event.destructure() {
                EventSummary::Key(_, code, value) if keyboard => {
                    // 1 = press, 2 = autorepeat. Releases must not emit.
                    if value == 1 || value == 2 {
                        let class = classify(code);
                        if class != KeyClass::Ignore {
                            sink(InputSignal::Key(class));
                        }
                    }
                }
                EventSummary::RelativeAxis(_, axis, value) if pointer => {
                    if axis == RelativeAxisCode::REL_X {
                        sink(InputSignal::Motion {
                            dx: value as f32,
                            dy: 0.0,
                        });
                    } else if axis == RelativeAxisCode::REL_Y {
                        sink(InputSignal::Motion {
                            dx: 0.0,
                            dy: value as f32,
                        });
                    }
                }
                _ => {}
            }
        }
    }
}
