use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

use async_channel::Sender;
use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::{Code, HotKey, Modifiers},
};
use klypse_domain::{HotkeyAction, KlypseError};

use crate::{HotkeyBinding, parse_accelerator};

pub struct X11HotkeyBackend {
    manager: GlobalHotKeyManager,
    registered: Vec<(HotkeyAction, HotKey)>,
    actions_by_id: Arc<RwLock<HashMap<u32, HotkeyAction>>>,
    stopped: Arc<AtomicBool>,
    listener: Option<JoinHandle<()>>,
}

impl X11HotkeyBackend {
    pub fn start(
        bindings: &[HotkeyBinding],
        actions: Sender<HotkeyAction>,
    ) -> Result<Self, KlypseError> {
        let registered = parse_bindings(bindings)?;
        let manager = GlobalHotKeyManager::new().map_err(hotkey_error)?;
        register_all(&manager, &registered)?;
        let actions_by_id = Arc::new(RwLock::new(binding_map(&registered)));
        let stopped = Arc::new(AtomicBool::new(false));
        let listener = std::thread::spawn({
            let actions_by_id = Arc::clone(&actions_by_id);
            let stopped = Arc::clone(&stopped);
            move || {
                while !stopped.load(Ordering::Relaxed) {
                    let Ok(event) =
                        GlobalHotKeyEvent::receiver().recv_timeout(Duration::from_millis(100))
                    else {
                        continue;
                    };
                    if event.state != HotKeyState::Pressed {
                        continue;
                    }
                    let action = actions_by_id
                        .read()
                        .ok()
                        .and_then(|mapping| mapping.get(&event.id).copied());
                    if let Some(action) = action {
                        let _ = actions.try_send(action);
                    }
                }
            }
        });
        Ok(Self {
            manager,
            registered,
            actions_by_id,
            stopped,
            listener: Some(listener),
        })
    }

    pub fn rebind(&mut self, bindings: &[HotkeyBinding]) -> Result<(), KlypseError> {
        let replacements = parse_bindings(bindings)?;
        unregister_all(&self.manager, &self.registered)?;
        if let Err(error) = register_all(&self.manager, &replacements) {
            let _ = register_all(&self.manager, &self.registered);
            return Err(error);
        }
        self.registered = replacements;
        if let Ok(mut mapping) = self.actions_by_id.write() {
            *mapping = binding_map(&self.registered);
        }
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), KlypseError> {
        let result = unregister_all(&self.manager, &self.registered);
        self.registered.clear();
        self.stopped.store(true, Ordering::Relaxed);
        if let Some(listener) = self.listener.take() {
            let _ = listener.join();
        }
        result
    }
}

impl Drop for X11HotkeyBackend {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

fn parse_bindings(bindings: &[HotkeyBinding]) -> Result<Vec<(HotkeyAction, HotKey)>, KlypseError> {
    let parsed = bindings
        .iter()
        .map(|binding| {
            accelerator_to_hotkey(&binding.accelerator).map(|hotkey| (binding.action, hotkey))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut ids = HashSet::new();
    if parsed.iter().any(|(_, hotkey)| !ids.insert(hotkey.id())) {
        return Err(KlypseError::InvalidRequest(
            "global shortcuts must be unique".into(),
        ));
    }
    Ok(parsed)
}

fn accelerator_to_hotkey(accelerator: &str) -> Result<HotKey, KlypseError> {
    let parsed = parse_accelerator(accelerator)?;
    let mut modifiers = Modifiers::empty();
    if parsed.control {
        modifiers |= Modifiers::CONTROL;
    }
    if parsed.shift {
        modifiers |= Modifiers::SHIFT;
    }
    if parsed.alt {
        modifiers |= Modifiers::ALT;
    }
    if parsed.super_key {
        modifiers |= Modifiers::SUPER;
    }
    let key = if parsed.key.eq_ignore_ascii_case("print") {
        Code::PrintScreen
    } else {
        parsed
            .key
            .parse::<HotKey>()
            .map(|hotkey| hotkey.key)
            .map_err(|_| KlypseError::InvalidRequest("unsupported shortcut key".into()))?
    };
    Ok(HotKey::new(Some(modifiers), key))
}

fn register_all(
    manager: &GlobalHotKeyManager,
    hotkeys: &[(HotkeyAction, HotKey)],
) -> Result<(), KlypseError> {
    let mut registered = Vec::new();
    for (_, hotkey) in hotkeys {
        if let Err(error) = manager.register(*hotkey) {
            for registered in registered {
                let _ = manager.unregister(registered);
            }
            return Err(hotkey_error(error));
        }
        registered.push(*hotkey);
    }
    Ok(())
}

fn unregister_all(
    manager: &GlobalHotKeyManager,
    hotkeys: &[(HotkeyAction, HotKey)],
) -> Result<(), KlypseError> {
    for (_, hotkey) in hotkeys {
        manager.unregister(*hotkey).map_err(hotkey_error)?;
    }
    Ok(())
}

fn binding_map(bindings: &[(HotkeyAction, HotKey)]) -> HashMap<u32, HotkeyAction> {
    bindings
        .iter()
        .map(|(action, hotkey)| (hotkey.id(), *action))
        .collect()
}

fn hotkey_error(error: impl std::fmt::Display) -> KlypseError {
    KlypseError::UnavailableCapability(format!("X11 global shortcut unavailable: {error}"))
}
