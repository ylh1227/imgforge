//! 撤销 / 重做历史栈。

use super::events::AnnotationCanvasEvent;
use super::types::{
    Annotation, AnnotationFingerprint, AnnotationKind, AnnotationPosition, AnnotationStyle,
};

#[derive(Debug, Clone)]
enum HistoryEntry {
    Create {
        fingerprint: AnnotationFingerprint,
        style: AnnotationStyle,
        content: String,
    },
    Update {
        id: i64,
        before: AnnotationPosition,
        after: AnnotationPosition,
    },
    Delete {
        snapshot: Annotation,
    },
    UpdateContent {
        id: i64,
        before: String,
        after: String,
    },
}

/// 内置标注历史栈（Ctrl+Z / Ctrl+Y）。
#[derive(Debug, Default)]
pub(crate) struct HistoryStack {
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
}

impl HistoryStack {
    pub fn record_create(
        &mut self,
        kind: AnnotationKind,
        position: AnnotationPosition,
        style: AnnotationStyle,
        content: String,
    ) {
        self.undo.push(HistoryEntry::Create {
            fingerprint: AnnotationFingerprint::from_parts(kind, &position),
            style,
            content,
        });
        self.redo.clear();
    }

    pub fn record_update(
        &mut self,
        id: i64,
        before: AnnotationPosition,
        after: AnnotationPosition,
    ) {
        self.undo.push(HistoryEntry::Update { id, before, after });
        self.redo.clear();
    }

    pub fn record_delete(&mut self, snapshot: Annotation) {
        self.undo.push(HistoryEntry::Delete { snapshot });
        self.redo.clear();
    }

    pub fn record_content(&mut self, id: i64, before: String, after: String) {
        self.undo
            .push(HistoryEntry::UpdateContent { id, before, after });
        self.redo.clear();
    }

    pub fn undo(&mut self, annotations: &[Annotation]) -> Option<AnnotationCanvasEvent> {
        let entry = self.undo.pop()?;
        let event = match &entry {
            HistoryEntry::Create { fingerprint, .. } => {
                let id = annotations.iter().find(|a| fingerprint.matches(a))?.id;
                AnnotationCanvasEvent::DeleteAnnotation { id }
            }
            HistoryEntry::Update { id, before, .. } => AnnotationCanvasEvent::UpdateAnnotation {
                id: *id,
                position: before.clone(),
            },
            HistoryEntry::Delete { snapshot } => AnnotationCanvasEvent::CreateAnnotation {
                kind: snapshot.kind,
                position: snapshot.position.clone(),
                style: snapshot.style.clone(),
                content: snapshot.content.clone(),
            },
            HistoryEntry::UpdateContent { id, before, .. } => {
                AnnotationCanvasEvent::UpdateAnnotationContent {
                    id: *id,
                    content: before.clone(),
                }
            }
        };
        self.redo.push(entry);
        Some(event)
    }

    pub fn redo(&mut self, _annotations: &[Annotation]) -> Option<AnnotationCanvasEvent> {
        let entry = self.redo.pop()?;
        let event = match &entry {
            HistoryEntry::Create {
                fingerprint,
                style,
                content,
            } => AnnotationCanvasEvent::CreateAnnotation {
                kind: fingerprint.kind,
                position: fingerprint.position.clone(),
                style: style.clone(),
                content: content.clone(),
            },
            HistoryEntry::Update { id, after, .. } => AnnotationCanvasEvent::UpdateAnnotation {
                id: *id,
                position: after.clone(),
            },
            HistoryEntry::Delete { snapshot } => {
                let id = snapshot.id;
                AnnotationCanvasEvent::DeleteAnnotation { id }
            }
            HistoryEntry::UpdateContent { id, after, .. } => {
                AnnotationCanvasEvent::UpdateAnnotationContent {
                    id: *id,
                    content: after.clone(),
                }
            }
        };
        self.undo.push(entry);
        Some(event)
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}
