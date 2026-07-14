use ashpd::{
    PortalError,
    desktop::{
        CreateSessionOptions,
        global_shortcuts::{BindShortcutsOptions, GlobalShortcuts, NewShortcut},
    },
};
use async_channel::Sender;
use futures_lite::{StreamExt, future};
use gtk::glib;
use klypse_domain::{HotkeyAction, KlypseError};

use crate::{HotkeyBinding, hotkey::action_from_id, parse_accelerator};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortalShortcutSpec {
    pub id: String,
    pub description: String,
    pub preferred_trigger: String,
}

pub fn portal_shortcut_specs(bindings: &[HotkeyBinding]) -> Vec<PortalShortcutSpec> {
    bindings
        .iter()
        .map(|binding| PortalShortcutSpec {
            id: binding.action.id().to_owned(),
            description: binding.description.clone(),
            preferred_trigger: portal_trigger(&binding.accelerator)
                .unwrap_or_else(|| binding.accelerator.clone()),
        })
        .collect()
}

pub fn validate_portal_bindings(
    requested: &[HotkeyBinding],
    bound_ids: &[String],
) -> Result<(), KlypseError> {
    if requested.is_empty()
        || requested
            .iter()
            .any(|binding| !bound_ids.iter().any(|id| id == binding.action.id()))
    {
        return Err(KlypseError::UnavailableCapability(
            "the global shortcuts portal returned no usable shortcuts".into(),
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PortalHotkeyClientError {
    Cancelled,
    PermissionDenied(String),
    Unavailable(String),
}

pub fn map_portal_hotkey_error(error: PortalHotkeyClientError) -> KlypseError {
    match error {
        PortalHotkeyClientError::Cancelled => KlypseError::Cancelled,
        PortalHotkeyClientError::PermissionDenied(message) => {
            KlypseError::PermissionDenied(message)
        }
        PortalHotkeyClientError::Unavailable(message) => {
            KlypseError::UnavailableCapability(message)
        }
    }
}

pub struct PortalHotkeyBackend {
    stop_sender: Sender<()>,
    task: Option<glib::JoinHandle<()>>,
}

impl PortalHotkeyBackend {
    pub async fn start(
        bindings: &[HotkeyBinding],
        actions: Sender<HotkeyAction>,
    ) -> Result<Self, KlypseError> {
        for binding in bindings {
            parse_accelerator(&binding.accelerator)?;
        }
        let proxy = GlobalShortcuts::new()
            .await
            .map_err(map_ashpd_hotkey_error)?;
        let session = proxy
            .create_session(CreateSessionOptions::default())
            .await
            .map_err(map_ashpd_hotkey_error)?;
        let mut activated = proxy
            .receive_activated()
            .await
            .map_err(map_ashpd_hotkey_error)?;
        let specs = portal_shortcut_specs(bindings);
        let shortcuts = specs
            .iter()
            .map(|shortcut| {
                NewShortcut::new(&shortcut.id, &shortcut.description)
                    .preferred_trigger(Some(shortcut.preferred_trigger.as_str()))
            })
            .collect::<Vec<_>>();
        let request = proxy
            .bind_shortcuts(&session, &shortcuts, None, BindShortcutsOptions::default())
            .await
            .map_err(map_ashpd_hotkey_error)?;
        let response = request.response().map_err(map_ashpd_hotkey_error)?;
        let bound_ids = response
            .shortcuts()
            .iter()
            .map(|shortcut| shortcut.id().to_owned())
            .collect::<Vec<_>>();
        validate_portal_bindings(bindings, &bound_ids)?;

        let (stop_sender, stop_receiver) = async_channel::bounded(1);
        let task = glib::spawn_future_local(async move {
            enum Event {
                Activated(Option<String>),
                Stop,
            }
            loop {
                let next_activation = async {
                    Event::Activated(
                        activated
                            .next()
                            .await
                            .map(|event| event.shortcut_id().to_owned()),
                    )
                };
                let stop = async {
                    let _ = stop_receiver.recv().await;
                    Event::Stop
                };
                match future::race(next_activation, stop).await {
                    Event::Activated(Some(id)) => {
                        if let Some(action) = action_from_id(&id) {
                            let _ = actions.try_send(action);
                        }
                    }
                    Event::Activated(None) | Event::Stop => break,
                }
            }
            let _ = session.close().await;
        });
        Ok(Self {
            stop_sender,
            task: Some(task),
        })
    }

    pub async fn stop(&mut self) {
        let _ = self.stop_sender.try_send(());
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

fn portal_trigger(accelerator: &str) -> Option<String> {
    let parsed = parse_accelerator(accelerator).ok()?;
    let mut parts = Vec::new();
    if parsed.control {
        parts.push("CTRL".to_owned());
    }
    if parsed.alt {
        parts.push("ALT".to_owned());
    }
    if parsed.shift {
        parts.push("SHIFT".to_owned());
    }
    if parsed.super_key {
        parts.push("LOGO".to_owned());
    }
    parts.push(parsed.key);
    Some(parts.join("+"))
}

fn map_ashpd_hotkey_error(error: ashpd::Error) -> KlypseError {
    let error = match error {
        ashpd::Error::Portal(PortalError::Cancelled(_)) => PortalHotkeyClientError::Cancelled,
        ashpd::Error::Portal(PortalError::NotAllowed(message)) => {
            PortalHotkeyClientError::PermissionDenied(message)
        }
        other if other.to_string().to_ascii_lowercase().contains("cancel") => {
            PortalHotkeyClientError::Cancelled
        }
        other => PortalHotkeyClientError::Unavailable(other.to_string()),
    };
    map_portal_hotkey_error(error)
}
