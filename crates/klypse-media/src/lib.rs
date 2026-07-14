mod recording;
mod thumbnail;

pub use recording::{
    PipelineSource, RecordingArtifact, RecordingMachine, RecordingState, VideoPipeline,
    VideoPipelineConfig,
};
pub use thumbnail::{MediaError, ThumbnailInfo, Thumbnailer};

pub const CRATE_NAME: &str = "klypse-media";
