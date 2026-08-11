use std::ffi::OsString;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum, error::ErrorKind};
use klypse_domain::{AppCommand, CaptureKind, CaptureRequest, CaptureTarget, RecordingRequest};

#[derive(Debug, Parser)]
#[command(
    name = "klypse",
    version,
    about = "Capture, record, and annotate your Linux desktop",
    long_about = "Capture, record, and annotate your Linux desktop.\n\n\
                  Running klypse with no arguments opens the gallery. The \
                  subcommands below are meant to be bound to keyboard shortcuts \
                  in your desktop settings, which is the supported way to get \
                  global shortcuts on GNOME, KDE and Xfce."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Open the gallery window
    Open,
    /// Take a screenshot
    Capture {
        /// What to capture
        #[arg(value_enum)]
        target: CaptureTargetArg,
    },
    /// Start recording a video or an animated GIF
    Record {
        /// Output format
        #[arg(value_enum)]
        kind: RecordingKindArg,
        /// What to record
        #[arg(value_enum, default_value = "area")]
        target: RecordingTargetArg,
    },
    /// Stop the recording in progress and save it
    Stop,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CaptureTargetArg {
    /// Drag to select a region of the screen
    Area,
    /// The whole desktop, every monitor included
    Screen,
    /// Pick a window to capture
    Window,
    /// The window currently focused
    ActiveWindow,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum RecordingKindArg {
    /// Silent WebM video
    Video,
    /// Animated GIF, capped by the duration set in Preferences
    Gif,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum RecordingTargetArg {
    /// Drag to select a region of the screen
    Area,
    /// The whole desktop, every monitor included
    Screen,
    /// Pick a window to record
    Window,
}

pub fn parse_from<I, T>(args: I) -> Result<AppCommand, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    match Cli::try_parse_from(args)?.command {
        None | Some(Command::Open) => Ok(AppCommand::Open),
        Some(Command::Capture { target }) => {
            Ok(AppCommand::Capture(CaptureRequest::new(target.into())))
        }
        Some(Command::Record { kind, target }) => {
            RecordingRequest::new(kind.into(), target.into(), None)
                .map(AppCommand::Record)
                .map_err(|error| Cli::command().error(ErrorKind::InvalidValue, error.to_string()))
        }
        Some(Command::Stop) => Ok(AppCommand::StopRecording),
    }
}

impl From<CaptureTargetArg> for CaptureTarget {
    fn from(value: CaptureTargetArg) -> Self {
        match value {
            CaptureTargetArg::Area => Self::Area,
            CaptureTargetArg::Screen => Self::Screen,
            CaptureTargetArg::Window => Self::Window,
            CaptureTargetArg::ActiveWindow => Self::ActiveWindow,
        }
    }
}

impl From<RecordingKindArg> for CaptureKind {
    fn from(value: RecordingKindArg) -> Self {
        match value {
            RecordingKindArg::Video => Self::Video,
            RecordingKindArg::Gif => Self::Gif,
        }
    }
}

impl From<RecordingTargetArg> for CaptureTarget {
    fn from(value: RecordingTargetArg) -> Self {
        match value {
            RecordingTargetArg::Area => Self::Area,
            RecordingTargetArg::Screen => Self::Screen,
            RecordingTargetArg::Window => Self::Window,
        }
    }
}
