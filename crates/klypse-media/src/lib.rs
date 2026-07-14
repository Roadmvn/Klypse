mod recording;
mod thumbnail;

pub use recording::{
    GifPipeline, GifPipelineConfig, PipelineSource, RecordingArtifact, RecordingMachine,
    RecordingState, VideoPipeline, VideoPipelineConfig, validate_gif,
};
pub use thumbnail::{MediaError, ThumbnailInfo, Thumbnailer};

pub const CRATE_NAME: &str = "klypse-media";
