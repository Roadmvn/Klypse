use std::{
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

use gstreamer as gst;

use crate::{MediaError, recording::pipeline::finalization_error};

/// A terminal stream event remains available until the pipeline is finalized.
/// In particular, observing EOS must not consume the event needed by `stop`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PipelineTerminalEvent {
    EndOfStream,
    Failed(String),
}

#[derive(Clone, Default)]
pub(crate) struct TerminalMonitor {
    state: Arc<(Mutex<Option<PipelineTerminalEvent>>, Condvar)>,
}

impl TerminalMonitor {
    pub fn install(bus: &gst::Bus) -> Self {
        let monitor = Self::default();
        bus.set_sync_handler({
            let monitor = monitor.clone();
            move |_, message| {
                match message.view() {
                    gst::MessageView::Eos(_) => monitor.publish(PipelineTerminalEvent::EndOfStream),
                    gst::MessageView::Error(error) => {
                        monitor.publish(PipelineTerminalEvent::Failed(format!(
                            "{} ({:?})",
                            error.error(),
                            error.debug()
                        )))
                    }
                    _ => {}
                }
                // This handler is the only bus consumer. Avoid accumulating
                // state/latency messages throughout a long recording.
                gst::BusSyncReply::Drop
            }
        });
        monitor
    }

    pub fn publish(&self, event: PipelineTerminalEvent) {
        let (state, changed) = &*self.state;
        let mut state = state.lock().unwrap_or_else(|error| error.into_inner());
        // An encoder can fail while draining samples after the sink posted EOS.
        // Never allow that successful stream-end message to hide a write error.
        if state.is_none() || matches!(event, PipelineTerminalEvent::Failed(_)) {
            *state = Some(event);
            changed.notify_all();
        }
    }

    pub fn event(&self) -> Option<PipelineTerminalEvent> {
        self.state
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub fn wait(&self, timeout: Duration) -> Result<(), MediaError> {
        let (state, changed) = &*self.state;
        let state = state.lock().unwrap_or_else(|error| error.into_inner());
        let (state, _) = changed
            .wait_timeout_while(state, timeout, |event| event.is_none())
            .unwrap_or_else(|error| error.into_inner());
        match state.as_ref() {
            Some(PipelineTerminalEvent::EndOfStream) => Ok(()),
            Some(PipelineTerminalEvent::Failed(error)) => Err(finalization_error(error)),
            None => Err(finalization_error(
                "timed out while finalizing the recording",
            )),
        }
    }
}
