use std::{collections::HashSet, sync::Arc};

use klypse_storage::{CaptureRecord, CaptureStore, DeleteMode, StorageError};
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GalleryPage {
    pub items: Vec<CaptureRecord>,
    pub has_more: bool,
}

pub struct GalleryController {
    store: Arc<dyn CaptureStore>,
    page_size: usize,
    items: Vec<CaptureRecord>,
    has_more: bool,
}

impl GalleryController {
    pub fn new(store: impl CaptureStore + 'static, page_size: usize) -> Result<Self, StorageError> {
        if !(1..=200).contains(&page_size) {
            return Err(StorageError::InvalidPageLimit(page_size));
        }
        Ok(Self {
            store: Arc::new(store),
            page_size,
            items: Vec::new(),
            has_more: true,
        })
    }

    pub fn load_initial(&mut self) -> Result<GalleryPage, StorageError> {
        self.items.clear();
        self.has_more = true;
        self.load_next()
    }

    pub fn load_next(&mut self) -> Result<GalleryPage, StorageError> {
        if !self.has_more {
            return Ok(self.page());
        }

        let request_limit = self.page_size.saturating_add(1).min(200);
        let mut records = self.store.list_page(self.items.len(), request_limit)?;
        let has_extra = if request_limit > self.page_size {
            records.len() > self.page_size
        } else if records.len() == self.page_size {
            !self
                .store
                .list_page(self.items.len() + self.page_size, 1)?
                .is_empty()
        } else {
            false
        };
        records.truncate(self.page_size);

        let mut known_ids = self
            .items
            .iter()
            .map(|record| record.id)
            .collect::<HashSet<_>>();
        self.items.extend(
            records
                .into_iter()
                .filter(|record| known_ids.insert(record.id)),
        );
        self.has_more = has_extra;
        Ok(self.page())
    }

    pub fn refresh(&mut self) -> Result<GalleryPage, StorageError> {
        self.load_initial()
    }

    pub fn delete(&mut self, id: &Uuid, mode: DeleteMode) -> Result<GalleryPage, StorageError> {
        self.store.delete(id, mode)?;
        self.items.retain(|record| &record.id != id);
        Ok(self.page())
    }

    pub fn delete_many(
        &mut self,
        ids: &[Uuid],
        mode: DeleteMode,
    ) -> Result<GalleryPage, StorageError> {
        if let Err(error) = self.store.delete_many(ids, mode) {
            let _ = self.load_initial();
            return Err(error);
        }
        self.load_initial()
    }

    pub fn all_ids(&self) -> Result<Vec<Uuid>, StorageError> {
        let mut ids = Vec::new();
        let mut known_ids = HashSet::new();
        let mut offset = 0_usize;
        loop {
            let records = self.store.list_page(offset, 200)?;
            let record_count = records.len();
            ids.extend(
                records
                    .into_iter()
                    .map(|record| record.id)
                    .filter(|id| known_ids.insert(*id)),
            );
            if record_count < 200 {
                break;
            }
            offset = offset.checked_add(record_count).ok_or_else(|| {
                StorageError::InvalidValue("gallery item offset is too large".into())
            })?;
        }
        Ok(ids)
    }

    pub fn clear(&mut self, mode: DeleteMode) -> Result<GalleryPage, StorageError> {
        if let Err(error) = self.store.delete_all(mode) {
            let _ = self.load_initial();
            return Err(error);
        }
        self.load_initial()
    }

    pub fn items(&self) -> &[CaptureRecord] {
        &self.items
    }

    pub const fn has_more(&self) -> bool {
        self.has_more
    }

    pub fn store(&self) -> Arc<dyn CaptureStore> {
        Arc::clone(&self.store)
    }

    fn page(&self) -> GalleryPage {
        GalleryPage {
            items: self.items.clone(),
            has_more: self.has_more,
        }
    }
}
