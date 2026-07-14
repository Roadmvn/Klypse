mod controller;

pub use controller::{GalleryController, GalleryPage};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GalleryEvent {
    Refresh,
    Select(Uuid),
}
