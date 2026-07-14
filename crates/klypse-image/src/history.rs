use std::collections::VecDeque;

use crate::{AnnotationDocument, ImageError, Layer, Rect};

pub const DEFAULT_HISTORY_LIMIT: usize = 100;

#[derive(Clone, Debug, PartialEq)]
pub enum DocumentCommand {
    AddLayer(Layer),
    RemoveLayer {
        layer: Layer,
        index: usize,
    },
    ReplaceLayer {
        before: Layer,
        after: Layer,
    },
    SetCrop {
        before: Option<Rect>,
        after: Option<Rect>,
    },
}

pub struct EditHistory {
    limit: usize,
    undo: VecDeque<DocumentCommand>,
    redo: Vec<DocumentCommand>,
}

impl Default for EditHistory {
    fn default() -> Self {
        Self::new(DEFAULT_HISTORY_LIMIT).expect("the default history limit is valid")
    }
}

impl EditHistory {
    pub fn new(limit: usize) -> Result<Self, ImageError> {
        if limit == 0 {
            return Err(ImageError::InvalidHistory(
                "history limit must be greater than zero",
            ));
        }
        Ok(Self {
            limit,
            undo: VecDeque::with_capacity(limit),
            redo: Vec::new(),
        })
    }

    pub fn execute(
        &mut self,
        document: &mut AnnotationDocument,
        command: DocumentCommand,
    ) -> Result<(), ImageError> {
        apply(document, &command, Direction::Forward)?;
        self.redo.clear();
        self.undo.push_back(command);
        if self.undo.len() > self.limit {
            self.undo.pop_front();
        }
        Ok(())
    }

    pub fn undo(&mut self, document: &mut AnnotationDocument) -> Result<(), ImageError> {
        let command = self
            .undo
            .pop_back()
            .ok_or(ImageError::InvalidHistory("nothing to undo"))?;
        if let Err(error) = apply(document, &command, Direction::Reverse) {
            self.undo.push_back(command);
            return Err(error);
        }
        self.redo.push(command);
        Ok(())
    }

    pub fn redo(&mut self, document: &mut AnnotationDocument) -> Result<(), ImageError> {
        let command = self
            .redo
            .pop()
            .ok_or(ImageError::InvalidHistory("nothing to redo"))?;
        if let Err(error) = apply(document, &command, Direction::Forward) {
            self.redo.push(command);
            return Err(error);
        }
        self.undo.push_back(command);
        Ok(())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[derive(Clone, Copy)]
enum Direction {
    Forward,
    Reverse,
}

fn apply(
    document: &mut AnnotationDocument,
    command: &DocumentCommand,
    direction: Direction,
) -> Result<(), ImageError> {
    let original = document.clone();
    let operation = match (command, direction) {
        (DocumentCommand::AddLayer(layer), Direction::Forward) => {
            document.layers.push(layer.clone());
            Ok(())
        }
        (DocumentCommand::AddLayer(layer), Direction::Reverse) => {
            remove_layer(document, &layer.id).map(|_| ())
        }
        (DocumentCommand::RemoveLayer { layer, .. }, Direction::Forward) => {
            remove_layer(document, &layer.id).map(|_| ())
        }
        (DocumentCommand::RemoveLayer { layer, index }, Direction::Reverse) => {
            document
                .layers
                .insert((*index).min(document.layers.len()), layer.clone());
            Ok(())
        }
        (DocumentCommand::ReplaceLayer { before, after }, Direction::Forward) => {
            replace_layer(document, &before.id, after.clone())
        }
        (DocumentCommand::ReplaceLayer { before, after }, Direction::Reverse) => {
            replace_layer(document, &after.id, before.clone())
        }
        (DocumentCommand::SetCrop { after, .. }, Direction::Forward) => {
            document.crop = *after;
            Ok(())
        }
        (DocumentCommand::SetCrop { before, .. }, Direction::Reverse) => {
            document.crop = *before;
            Ok(())
        }
    };
    if let Err(error) = operation.and_then(|()| document.validate()) {
        *document = original;
        return Err(error);
    }
    Ok(())
}

fn remove_layer(document: &mut AnnotationDocument, id: &str) -> Result<Layer, ImageError> {
    let index = document
        .layers
        .iter()
        .position(|layer| layer.id == id)
        .ok_or_else(|| ImageError::InvalidDocument("layer to remove was not found".into()))?;
    Ok(document.layers.remove(index))
}

fn replace_layer(
    document: &mut AnnotationDocument,
    id: &str,
    replacement: Layer,
) -> Result<(), ImageError> {
    let layer = document
        .layers
        .iter_mut()
        .find(|layer| layer.id == id)
        .ok_or_else(|| ImageError::InvalidDocument("layer to replace was not found".into()))?;
    *layer = replacement;
    Ok(())
}
