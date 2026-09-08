mod gif;
mod pipeline;
mod state;
mod terminal;
mod video;

pub use gif::{GifPipeline, GifPipelineConfig, validate_gif};
pub use pipeline::{PipelineSource, RecordingArtifact};
pub use state::{RecordingMachine, RecordingState};
pub use terminal::PipelineTerminalEvent;
pub use video::{VideoPipeline, VideoPipelineConfig};
