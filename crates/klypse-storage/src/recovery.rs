use std::{
    collections::HashSet,
    fs::{self, File},
    io::BufReader,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use chrono::{DateTime, Utc};
use gstreamer as gst;
use gstreamer_pbutils as gst_pbutils;
use image::AnimationDecoder;
use klypse_domain::{CaptureKind, CaptureTarget, DisplayServer};
use klypse_media::Thumbnailer;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    AppPaths, AtomicCaptureFile, CaptureRecord, CaptureStore, NewCaptureRecord, StorageError,
};

const RECOVERY_MARKER_NAME: &str = ".klypse-session.json";
const THUMBNAIL_EDGE: u32 = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoverableFile {
    pub id: Uuid,
    pub path: PathBuf,
    pub kind: CaptureKind,
    pub width: u32,
    pub height: u32,
    pub duration: Option<Duration>,
    pub created_at: DateTime<Utc>,
    pub target: CaptureTarget,
    pub backend: DisplayServer,
    marker_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidRecoveryFile {
    pub path: PathBuf,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RecoveryReport {
    pub recoverable: Vec<RecoverableFile>,
    pub unrecoverable: Vec<InvalidRecoveryFile>,
    pub missing_files: Vec<CaptureRecord>,
    pub stale_thumbnails: Vec<PathBuf>,
}

#[derive(Clone, Debug, Deserialize)]
struct RecordingMarker {
    session_id: Uuid,
    kind: CaptureKind,
    backend: DisplayServer,
    temporary_path: PathBuf,
    target: CaptureTarget,
    started_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug)]
struct MediaMetadata {
    kind: CaptureKind,
    width: u32,
    height: u32,
    duration: Option<Duration>,
}

pub struct Reconciler {
    paths: AppPaths,
    store: Arc<dyn CaptureStore>,
}

impl Reconciler {
    pub fn new(paths: AppPaths, store: Arc<dyn CaptureStore>) -> Self {
        Self { paths, store }
    }

    pub fn scan(&self) -> Result<RecoveryReport, StorageError> {
        self.paths.ensure()?;
        let records = all_records(self.store.as_ref())?;
        let mut report = RecoveryReport {
            missing_files: records
                .iter()
                .filter(|record| !record.path.is_file())
                .cloned()
                .collect(),
            ..RecoveryReport::default()
        };
        report.stale_thumbnails = stale_thumbnails(&self.paths, &records)?;

        let marker_path = self.paths.temporary.join(RECOVERY_MARKER_NAME);
        let marker = if marker_path.exists() {
            match read_marker(&marker_path) {
                Ok(marker) => Some(marker),
                Err(error) => {
                    report.unrecoverable.push(InvalidRecoveryFile {
                        path: marker_path.clone(),
                        reason: error.to_string(),
                    });
                    None
                }
            }
        } else {
            None
        };
        let marker_target = marker.as_ref().and_then(|marker| {
            match self.owned_media_path(&marker.temporary_path) {
                Ok(path) if path.starts_with(canonical_directory(&self.paths.temporary).ok()?) => {
                    Some(path)
                }
                Ok(_) | Err(_) => None,
            }
        });
        if marker.is_some() && marker_target.is_none() {
            report.unrecoverable.push(InvalidRecoveryFile {
                path: marker_path.clone(),
                reason: "recording marker points outside the Klypse temporary directory or to a missing file".into(),
            });
        }

        let known_paths = records
            .iter()
            .filter_map(|record| record.path.canonicalize().ok())
            .collect::<HashSet<_>>();
        for directory in [&self.paths.temporary, &self.paths.orphans] {
            for path in directory_files(directory)? {
                if path.file_name().and_then(|name| name.to_str()) == Some(RECOVERY_MARKER_NAME) {
                    continue;
                }
                let canonical = match self.owned_media_path(&path) {
                    Ok(path) => path,
                    Err(error) => {
                        report.unrecoverable.push(InvalidRecoveryFile {
                            path,
                            reason: error.to_string(),
                        });
                        continue;
                    }
                };
                if known_paths.contains(&canonical) {
                    continue;
                }
                let attached_marker = marker.as_ref().filter(|_| {
                    marker_target
                        .as_ref()
                        .is_some_and(|target| target == &canonical)
                });
                match inspect_media(&canonical) {
                    Ok(media) => {
                        if let Some(marker) = attached_marker
                            && marker.kind != media.kind
                        {
                            report.unrecoverable.push(InvalidRecoveryFile {
                                path: canonical,
                                reason: "recording marker kind does not match its media file"
                                    .into(),
                            });
                            continue;
                        }
                        let id = attached_marker
                            .map(|marker| marker.session_id)
                            .or_else(|| {
                                canonical
                                    .file_stem()
                                    .and_then(|stem| stem.to_str())
                                    .and_then(|stem| Uuid::parse_str(stem).ok())
                            })
                            .unwrap_or_else(Uuid::new_v4);
                        if self.store.get(&id)?.is_some() {
                            report.unrecoverable.push(InvalidRecoveryFile {
                                path: canonical,
                                reason: format!("capture {id} already exists in the gallery"),
                            });
                            continue;
                        }
                        report.recoverable.push(RecoverableFile {
                            id,
                            path: canonical,
                            kind: attached_marker.map_or(media.kind, |marker| marker.kind),
                            width: media.width,
                            height: media.height,
                            duration: media.duration,
                            created_at: attached_marker
                                .map(|marker| marker.started_at)
                                .unwrap_or_else(|| modified_at(&path)),
                            target: attached_marker
                                .map_or(CaptureTarget::Area, |marker| marker.target),
                            backend: attached_marker
                                .map_or(DisplayServer::X11, |marker| marker.backend),
                            marker_path: attached_marker.map(|_| marker_path.clone()),
                        });
                    }
                    Err(error) => report.unrecoverable.push(InvalidRecoveryFile {
                        path: canonical,
                        reason: error.to_string(),
                    }),
                }
            }
        }
        report
            .recoverable
            .sort_by_key(|candidate| candidate.created_at);
        report
            .unrecoverable
            .sort_by(|left, right| left.path.cmp(&right.path));
        report.missing_files.sort_by_key(|record| record.created_at);
        report.stale_thumbnails.sort();
        Ok(report)
    }

    pub fn restore(&self, candidate: &RecoverableFile) -> Result<CaptureRecord, StorageError> {
        let source = self.owned_media_path(&candidate.path)?;
        if let Some(marker_path) = &candidate.marker_path {
            self.validate_matching_marker(marker_path, candidate, &source)?;
        }
        let media = inspect_media(&source)?;
        if media.kind != candidate.kind {
            return Err(StorageError::Recovery(
                "the recovery file kind changed after it was scanned".into(),
            ));
        }
        if self.store.get(&candidate.id)?.is_some() {
            return Err(StorageError::Recovery(format!(
                "capture {} already exists in the gallery",
                candidate.id
            )));
        }
        let extension = extension_for(candidate.kind);
        let destination = self
            .paths
            .captures
            .join(format!("{}.{}", candidate.id, extension));
        if destination.exists() {
            return Err(StorageError::Recovery(format!(
                "capture destination {} already exists",
                destination.display()
            )));
        }

        let mut output = AtomicCaptureFile::new(&self.paths, candidate.id, extension)?;
        let mut input = File::open(&source)?;
        std::io::copy(&mut input, &mut output)?;
        let committed = output.commit()?;
        let file_size = fs::metadata(&committed)?.len();
        let inserted = self.store.insert(NewCaptureRecord {
            id: candidate.id,
            kind: candidate.kind,
            path: committed.clone(),
            original_path: None,
            thumbnail_path: None,
            created_at: candidate.created_at,
            width: media.width,
            height: media.height,
            duration: media.duration,
            file_size,
            target: candidate.target,
            backend: candidate.backend,
            annotation_json: None,
        });
        let mut record = match inserted {
            Ok(record) => record,
            Err(error) => {
                let _ = fs::remove_file(&committed);
                return Err(error);
            }
        };

        let thumbnail = self.paths.thumbnails.join(format!("{}.png", record.id));
        if Thumbnailer::new(THUMBNAIL_EDGE)
            .generate(&record.path, &thumbnail)
            .is_ok()
            && self.store.set_thumbnail(&record.id, &thumbnail).is_ok()
        {
            record.thumbnail_path = Some(thumbnail);
        }
        fs::remove_file(&source)?;
        sync_parent(&source)?;
        if let Some(marker_path) = &candidate.marker_path {
            self.remove_marker(marker_path)?;
        }
        Ok(record)
    }

    pub fn discard(&self, candidate: &RecoverableFile) -> Result<(), StorageError> {
        let source = self.owned_media_path(&candidate.path)?;
        if let Some(marker_path) = &candidate.marker_path {
            self.validate_matching_marker(marker_path, candidate, &source)?;
        }
        self.discard_path(&candidate.path)?;
        if let Some(marker_path) = &candidate.marker_path {
            self.remove_marker(marker_path)?;
        }
        Ok(())
    }

    pub fn discard_path(&self, path: &Path) -> Result<(), StorageError> {
        let path = self.owned_media_path(path)?;
        fs::remove_file(&path)?;
        sync_parent(&path)
    }

    pub fn discard_stale_thumbnail(&self, path: &Path) -> Result<(), StorageError> {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(StorageError::Recovery(
                "stale thumbnail must be a regular file".into(),
            ));
        }
        let canonical = path.canonicalize()?;
        if !canonical.starts_with(canonical_directory(&self.paths.thumbnails)?) {
            return Err(StorageError::Recovery(
                "stale thumbnail is outside the Klypse cache".into(),
            ));
        }
        fs::remove_file(&canonical)?;
        sync_parent(&canonical)
    }

    pub fn relocate_missing(
        &self,
        id: &Uuid,
        replacement: &Path,
    ) -> Result<CaptureRecord, StorageError> {
        let previous = self
            .store
            .get(id)?
            .ok_or(StorageError::CaptureNotFound(*id))?;
        if previous.path.exists() {
            return Err(StorageError::Recovery(
                "only captures with missing files can be relocated".into(),
            ));
        }
        let metadata = fs::symlink_metadata(replacement)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(StorageError::Recovery(
                "the replacement capture must be a regular file".into(),
            ));
        }
        let replacement = replacement.canonicalize()?;
        let media = inspect_media(&replacement)?;
        if media.kind != previous.kind {
            return Err(StorageError::Recovery(
                "the replacement media kind does not match the gallery item".into(),
            ));
        }
        let mut record = self.store.relocate(
            id,
            &replacement,
            media.width,
            media.height,
            media.duration,
            fs::metadata(&replacement)?.len(),
        )?;
        if let Some(old_thumbnail) = previous.thumbnail_path
            && is_owned_existing_file(&old_thumbnail, &self.paths.thumbnails)
        {
            let _ = fs::remove_file(old_thumbnail);
        }
        let thumbnail = self.paths.thumbnails.join(format!("{}.png", record.id));
        if Thumbnailer::new(THUMBNAIL_EDGE)
            .generate(&record.path, &thumbnail)
            .is_ok()
            && self.store.set_thumbnail(&record.id, &thumbnail).is_ok()
        {
            record.thumbnail_path = Some(thumbnail);
        }
        Ok(record)
    }

    pub fn store(&self) -> Arc<dyn CaptureStore> {
        Arc::clone(&self.store)
    }

    fn owned_media_path(&self, path: &Path) -> Result<PathBuf, StorageError> {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(StorageError::Recovery(
                "recovery paths must be regular files, not links".into(),
            ));
        }
        let canonical = path.canonicalize()?;
        let temporary = canonical_directory(&self.paths.temporary)?;
        let orphans = canonical_directory(&self.paths.orphans)?;
        if !canonical.starts_with(&temporary) && !canonical.starts_with(&orphans) {
            return Err(StorageError::Recovery(
                "recovery path is outside Klypse-owned directories".into(),
            ));
        }
        Ok(canonical)
    }

    fn validate_matching_marker(
        &self,
        marker_path: &Path,
        candidate: &RecoverableFile,
        canonical_source: &Path,
    ) -> Result<(), StorageError> {
        let expected = self.paths.temporary.join(RECOVERY_MARKER_NAME);
        if marker_path != expected {
            return Err(StorageError::Recovery(
                "recovery marker path is not owned by Klypse".into(),
            ));
        }
        let marker = read_marker(marker_path)?;
        if marker.session_id != candidate.id
            || marker.temporary_path.canonicalize()?.as_path() != canonical_source
        {
            return Err(StorageError::Recovery(
                "recovery marker changed after it was scanned".into(),
            ));
        }
        Ok(())
    }

    fn remove_marker(&self, marker_path: &Path) -> Result<(), StorageError> {
        if marker_path != self.paths.temporary.join(RECOVERY_MARKER_NAME) {
            return Err(StorageError::Recovery(
                "recovery marker path is not owned by Klypse".into(),
            ));
        }
        fs::remove_file(marker_path)?;
        sync_parent(marker_path)
    }
}

fn all_records(store: &dyn CaptureStore) -> Result<Vec<CaptureRecord>, StorageError> {
    let mut records = Vec::new();
    loop {
        let page = store.list_page(records.len(), 200)?;
        let finished = page.len() < 200;
        records.extend(page);
        if finished {
            return Ok(records);
        }
    }
}

fn stale_thumbnails(
    paths: &AppPaths,
    records: &[CaptureRecord],
) -> Result<Vec<PathBuf>, StorageError> {
    let referenced = records
        .iter()
        .filter_map(|record| record.thumbnail_path.as_ref())
        .filter_map(|path| path.canonicalize().ok())
        .collect::<HashSet<_>>();
    Ok(directory_files(&paths.thumbnails)?
        .into_iter()
        .filter_map(|path| path.canonicalize().ok())
        .filter(|path| !referenced.contains(path))
        .collect())
}

fn directory_files(directory: &Path) -> Result<Vec<PathBuf>, StorageError> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_file() && !file_type.is_symlink() {
            files.push(entry.path());
        }
    }
    Ok(files)
}

fn read_marker(path: &Path) -> Result<RecordingMarker, StorageError> {
    serde_json::from_reader(File::open(path)?)
        .map_err(|error| StorageError::Recovery(format!("invalid recording marker: {error}")))
}

fn inspect_media(path: &Path) -> Result<MediaMetadata, StorageError> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => inspect_png(path),
        Some("gif") => inspect_gif(path),
        Some("webm") => inspect_webm(path),
        _ => Err(StorageError::Recovery(
            "only PNG, GIF, and WebM recovery files are supported".into(),
        )),
    }
}

fn inspect_png(path: &Path) -> Result<MediaMetadata, StorageError> {
    let image = image::ImageReader::open(path)?
        .with_guessed_format()
        .map_err(StorageError::Io)?
        .decode()
        .map_err(media_error)?;
    if image.width() == 0 || image.height() == 0 {
        return Err(StorageError::Recovery(
            "recovery image has invalid dimensions".into(),
        ));
    }
    Ok(MediaMetadata {
        kind: CaptureKind::Screenshot,
        width: image.width(),
        height: image.height(),
        duration: None,
    })
}

fn inspect_gif(path: &Path) -> Result<MediaMetadata, StorageError> {
    let decoder = image::codecs::gif::GifDecoder::new(BufReader::new(File::open(path)?))
        .map_err(media_error)?;
    let mut frames = decoder.into_frames();
    let first = frames
        .next()
        .transpose()
        .map_err(media_error)?
        .ok_or_else(|| {
            StorageError::Recovery("the recovery GIF contains no complete frames".into())
        })?;
    let width = first.buffer().width();
    let height = first.buffer().height();
    let duration = frames.try_fold(frame_duration(&first), |duration, frame| {
        let frame = frame.map_err(media_error)?;
        Ok::<_, StorageError>(duration.saturating_add(frame_duration(&frame)))
    })?;
    Ok(MediaMetadata {
        kind: CaptureKind::Gif,
        width,
        height,
        duration: Some(duration),
    })
}

fn frame_duration(frame: &image::Frame) -> Duration {
    let (numerator, denominator) = frame.delay().numer_denom_ms();
    let millis = if denominator == 0 {
        0
    } else {
        u64::from(numerator) / u64::from(denominator)
    };
    Duration::from_millis(millis)
}

fn inspect_webm(path: &Path) -> Result<MediaMetadata, StorageError> {
    gst::init().map_err(media_error)?;
    let canonical = path.canonicalize()?;
    let uri = gst::glib::filename_to_uri(&canonical, None).map_err(media_error)?;
    let discoverer =
        gst_pbutils::Discoverer::new(gst::ClockTime::from_seconds(10)).map_err(media_error)?;
    let info = discoverer.discover_uri(&uri).map_err(media_error)?;
    let video = info.video_streams().into_iter().next().ok_or_else(|| {
        StorageError::Recovery("the recovery WebM contains no video stream".into())
    })?;
    let duration = info
        .duration()
        .map(|duration| Duration::from_nanos(duration.nseconds()));
    if video.width() == 0 || video.height() == 0 || duration.is_none() {
        return Err(StorageError::Recovery(
            "the recovery WebM has incomplete metadata".into(),
        ));
    }
    Ok(MediaMetadata {
        kind: CaptureKind::Video,
        width: video.width(),
        height: video.height(),
        duration,
    })
}

fn modified_at(path: &Path) -> DateTime<Utc> {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .map(DateTime::<Utc>::from)
        .unwrap_or_else(|_| DateTime::<Utc>::from(SystemTime::UNIX_EPOCH))
}

fn canonical_directory(path: &Path) -> Result<PathBuf, StorageError> {
    path.canonicalize().map_err(StorageError::Io)
}

fn is_owned_existing_file(path: &Path, root: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| {
        metadata.is_file()
            && !metadata.file_type().is_symlink()
            && path
                .canonicalize()
                .ok()
                .zip(root.canonicalize().ok())
                .is_some_and(|(path, root)| path.starts_with(root))
    })
}

fn sync_parent(path: &Path) -> Result<(), StorageError> {
    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

const fn extension_for(kind: CaptureKind) -> &'static str {
    match kind {
        CaptureKind::Screenshot => "png",
        CaptureKind::Video => "webm",
        CaptureKind::Gif => "gif",
    }
}

fn media_error(error: impl std::fmt::Display) -> StorageError {
    StorageError::Recovery(error.to_string())
}
