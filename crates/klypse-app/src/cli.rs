use std::ffi::OsString;

use clap::{CommandFactory, Parser, Subcommand, ValueEnum, error::ErrorKind};
use klypse_domain::{AppCommand, CaptureKind, CaptureRequest, CaptureTarget, RecordingRequest};

#[derive(Debug, Parser)]
#[command(
    name = "klypse",
    version,
    about = "Capture and annotate your Linux desktop"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Open,
    Capture {
        #[arg(value_enum)]
        target: CaptureTargetArg,
    },
    Record {
        #[arg(value_enum)]
        kind: RecordingKindArg,
        #[arg(value_enum, default_value = "area")]
        target: RecordingTargetArg,
    },
    Stop,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum CaptureTargetArg {
    Area,
    Screen,
    Window,
    ActiveWindow,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum RecordingKindArg {
    Video,
    Gif,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum RecordingTargetArg {
    Area,
    Screen,
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
