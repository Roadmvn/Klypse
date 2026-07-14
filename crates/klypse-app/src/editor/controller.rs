use klypse_image::{
    AnnotationDocument, DocumentCommand, EditHistory, ImageError, Layer, LayerKind, Point, Rect,
    RedactionMode, Rgba, Stroke, ViewportTransform,
};

const MIN_ZOOM: f64 = 0.1;
const MAX_ZOOM: f64 = 8.0;
const DEFAULT_STROKE_WIDTH: f64 = 3.0;
const DEFAULT_TEXT_SIZE: f64 = 24.0;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EditorTool {
    #[default]
    Rectangle,
    Ellipse,
    Line,
    Arrow,
    Text,
    Freehand,
    Pixelate,
    Blur,
    Crop,
}

pub struct EditorController {
    document: AnnotationDocument,
    saved_document: AnnotationDocument,
    history: EditHistory,
    active_tool: EditorTool,
    gesture: Option<Gesture>,
    draft_layer: Option<Layer>,
    pending_text_origin: Option<Point>,
    transform: ViewportTransform,
    zoom: f64,
    color: Rgba,
    stroke_width: f64,
    text_size: f64,
    font: String,
    next_layer_id: u64,
}

#[derive(Clone, Debug)]
struct Gesture {
    start: Point,
    current: Point,
    freehand: Vec<Point>,
}

impl EditorController {
    pub fn new(width: u32, height: u32) -> Result<Self, ImageError> {
        Self::from_document(AnnotationDocument::new(width, height)?)
    }

    pub fn from_document(document: AnnotationDocument) -> Result<Self, ImageError> {
        document.validate()?;
        let color = Rgba::new(0.93, 0.12, 0.18, 1.0)?;
        let mut controller = Self {
            saved_document: document.clone(),
            document,
            history: EditHistory::default(),
            active_tool: EditorTool::default(),
            gesture: None,
            draft_layer: None,
            pending_text_origin: None,
            transform: ViewportTransform::new(1.0, Point::new(0.0, 0.0)?)?,
            zoom: 1.0,
            color,
            stroke_width: DEFAULT_STROKE_WIDTH,
            text_size: DEFAULT_TEXT_SIZE,
            font: "Sans".into(),
            next_layer_id: 1,
        };
        controller.refresh_transform()?;
        Ok(controller)
    }

    pub fn document(&self) -> &AnnotationDocument {
        &self.document
    }

    pub fn draft_layer(&self) -> Option<&Layer> {
        self.draft_layer.as_ref()
    }

    pub const fn active_tool(&self) -> EditorTool {
        self.active_tool
    }

    pub fn set_tool(&mut self, tool: EditorTool) {
        self.cancel_current_action();
        self.active_tool = tool;
    }

    pub fn pointer_down(&mut self, view_point: Point) -> Result<(), ImageError> {
        self.cancel_current_action();
        let point = self.image_point(view_point)?;
        self.gesture = Some(Gesture {
            start: point,
            current: point,
            freehand: vec![point],
        });
        self.refresh_draft()
    }

    pub fn pointer_move(&mut self, view_point: Point) -> Result<(), ImageError> {
        let point = self.image_point(view_point)?;
        let Some(gesture) = self.gesture.as_mut() else {
            return Ok(());
        };
        gesture.current = point;
        if self.active_tool == EditorTool::Freehand
            && gesture.freehand.last().is_none_or(|previous| {
                (previous.x - point.x).abs() >= 0.25 || (previous.y - point.y).abs() >= 0.25
            })
        {
            gesture.freehand.push(point);
        }
        self.refresh_draft()
    }

    pub fn pointer_up(&mut self, view_point: Point) -> Result<(), ImageError> {
        self.pointer_move(view_point)?;
        let Some(gesture) = self.gesture.take() else {
            return Ok(());
        };
        self.draft_layer = None;

        match self.active_tool {
            EditorTool::Text => {
                self.pending_text_origin = Some(gesture.current);
                Ok(())
            }
            EditorTool::Crop => {
                let Some(rect) = normalized_rect(gesture.start, gesture.current) else {
                    return Ok(());
                };
                let before = self.document.crop;
                self.history.execute(
                    &mut self.document,
                    DocumentCommand::SetCrop {
                        before,
                        after: Some(rect),
                    },
                )?;
                self.refresh_transform()
            }
            _ => {
                let id = self.allocate_layer_id();
                let Some(layer) = self.layer_for_gesture(&gesture, id)? else {
                    return Ok(());
                };
                self.history
                    .execute(&mut self.document, DocumentCommand::AddLayer(layer))
            }
        }
    }

    pub fn set_text(&mut self, text: &str) -> Result<bool, ImageError> {
        let Some(origin) = self.pending_text_origin.take() else {
            return Ok(false);
        };
        if text.trim().is_empty() {
            return Ok(false);
        }
        let layer = Layer::new(
            self.allocate_layer_id(),
            LayerKind::Text {
                origin,
                text: text.to_owned(),
                font: self.font.clone(),
                size: self.text_size,
                color: self.color,
            },
        );
        self.history
            .execute(&mut self.document, DocumentCommand::AddLayer(layer))?;
        Ok(true)
    }

    pub fn cancel_current_action(&mut self) {
        self.gesture = None;
        self.draft_layer = None;
        self.pending_text_origin = None;
    }

    pub fn undo(&mut self) -> Result<(), ImageError> {
        self.cancel_current_action();
        self.history.undo(&mut self.document)?;
        self.refresh_transform()
    }

    pub fn redo(&mut self) -> Result<(), ImageError> {
        self.cancel_current_action();
        self.history.redo(&mut self.document)?;
        self.refresh_transform()
    }

    pub fn delete_last_layer(&mut self) -> Result<bool, ImageError> {
        let Some((index, layer)) = self
            .document
            .layers
            .len()
            .checked_sub(1)
            .map(|index| (index, self.document.layers[index].clone()))
        else {
            return Ok(false);
        };
        self.history.execute(
            &mut self.document,
            DocumentCommand::RemoveLayer { layer, index },
        )?;
        Ok(true)
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn is_dirty(&self) -> bool {
        self.document != self.saved_document
    }

    pub fn mark_saved(&mut self) {
        self.saved_document = self.document.clone();
    }

    pub const fn zoom(&self) -> f64 {
        self.zoom
    }

    pub fn set_zoom(&mut self, zoom: f64) -> Result<(), ImageError> {
        if !zoom.is_finite() || !(MIN_ZOOM..=MAX_ZOOM).contains(&zoom) {
            return Err(ImageError::InvalidGeometry(
                "editor zoom must be finite and within 0.1..=8.0",
            ));
        }
        self.zoom = zoom;
        self.refresh_transform()?;
        self.cancel_current_action();
        Ok(())
    }

    pub fn zoom_by(&mut self, factor: f64) -> Result<(), ImageError> {
        self.set_zoom((self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM))
    }

    pub fn set_color(&mut self, color: Rgba) -> Result<(), ImageError> {
        color.validate()?;
        self.color = color;
        Ok(())
    }

    pub fn set_stroke_width(&mut self, width: f64) -> Result<(), ImageError> {
        Stroke::new(self.color, width)?;
        self.stroke_width = width;
        Ok(())
    }

    pub fn set_text_style(&mut self, font: impl Into<String>, size: f64) -> Result<(), ImageError> {
        let font = font.into();
        let probe = Layer::new(
            "text-style-probe",
            LayerKind::Text {
                origin: Point::new(0.0, 0.0)?,
                text: "probe".into(),
                font: font.clone(),
                size,
                color: self.color,
            },
        );
        let mut document = AnnotationDocument::new(1, 1)?;
        document.layers.push(probe);
        document.validate()?;
        self.font = font;
        self.text_size = size;
        Ok(())
    }

    fn image_point(&self, view_point: Point) -> Result<Point, ImageError> {
        Point::new(view_point.x, view_point.y)?;
        let image = self.transform.view_to_image(view_point);
        Point::new(
            image.x.clamp(0.0, f64::from(self.document.original_width)),
            image.y.clamp(0.0, f64::from(self.document.original_height)),
        )
    }

    fn refresh_transform(&mut self) -> Result<(), ImageError> {
        let (crop_x, crop_y) = self
            .document
            .crop
            .map(|crop| (crop.x, crop.y))
            .unwrap_or((0.0, 0.0));
        self.transform = ViewportTransform::new(
            self.zoom,
            Point::new(-crop_x * self.zoom, -crop_y * self.zoom)?,
        )?;
        Ok(())
    }

    fn refresh_draft(&mut self) -> Result<(), ImageError> {
        self.draft_layer = self
            .gesture
            .as_ref()
            .map(|gesture| self.layer_for_gesture(gesture, "draft".into()))
            .transpose()?
            .flatten();
        Ok(())
    }

    fn layer_for_gesture(
        &self,
        gesture: &Gesture,
        id: String,
    ) -> Result<Option<Layer>, ImageError> {
        let stroke = Stroke::new(self.color, self.stroke_width)?;
        let rect = normalized_rect(gesture.start, gesture.current);
        let kind = match self.active_tool {
            EditorTool::Rectangle => rect.map(|rect| LayerKind::Rectangle {
                rect,
                stroke,
                fill: None,
            }),
            EditorTool::Ellipse => rect.map(|rect| LayerKind::Ellipse {
                rect,
                stroke,
                fill: None,
            }),
            EditorTool::Line => Some(LayerKind::Line {
                start: gesture.start,
                end: gesture.current,
                stroke,
            }),
            EditorTool::Arrow => Some(LayerKind::Arrow {
                start: gesture.start,
                end: gesture.current,
                stroke,
                head_length: (self.stroke_width * 4.0).max(10.0),
            }),
            EditorTool::Freehand => Some(LayerKind::Freehand {
                points: gesture.freehand.clone(),
                stroke,
            }),
            EditorTool::Pixelate => rect.map(|rect| LayerKind::Redaction {
                rect,
                mode: RedactionMode::Pixelate { block_size: 12 },
            }),
            EditorTool::Blur => rect.map(|rect| LayerKind::Redaction {
                rect,
                mode: RedactionMode::Blur { radius: 8 },
            }),
            EditorTool::Text | EditorTool::Crop => None,
        };
        Ok(kind.map(|kind| Layer::new(id, kind)))
    }

    fn allocate_layer_id(&mut self) -> String {
        let id = format!("layer-{}", self.next_layer_id);
        self.next_layer_id += 1;
        id
    }
}

fn normalized_rect(start: Point, end: Point) -> Option<Rect> {
    Rect::from_points(start, end).ok()
}
