pub mod editor;
pub mod gallery;
pub mod recording;
pub mod recovery;
pub mod region_overlay;
pub mod settings;
pub mod window;

pub(crate) fn set_accessible_label(widget: &impl gtk::prelude::IsA<gtk::Accessible>, label: &str) {
    use gtk::prelude::AccessibleExtManual;

    widget.update_property(&[gtk::accessible::Property::Label(label)]);
}
