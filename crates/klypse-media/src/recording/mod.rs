mod pipeline;
mod state;
mod video;

pub use pipeline::{PipelineSource, RecordingArtifact};
pub use state::{RecordingMachine, RecordingState};
pub use video::{VideoPipeline, VideoPipelineConfig};
