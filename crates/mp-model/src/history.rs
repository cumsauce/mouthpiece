use crate::Project;

const MAX_UNDO: usize = 256;

/// Undo stack of whole-project states. Clips are small metadata and audio is not part of the
/// project, so a full clone stays cheap even with thousands of word clips.
#[derive(Default)]
pub struct History {
    undo: Vec<(String, Project)>,
    redo: Vec<(String, Project)>,
}

impl History {
    /// Record `before` as the state an undo returns to.
    pub fn push(&mut self, label: impl Into<String>, before: Project) {
        self.undo.push((label.into(), before));
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Returns the undone edit's label.
    pub fn undo(&mut self, current: &mut Project) -> Option<String> {
        let (label, state) = self.undo.pop()?;
        self.redo.push((label.clone(), std::mem::replace(current, state)));
        Some(label)
    }

    pub fn redo(&mut self, current: &mut Project) -> Option<String> {
        let (label, state) = self.redo.pop()?;
        self.undo.push((label.clone(), std::mem::replace(current, state)));
        Some(label)
    }

    pub fn labels(&self) -> impl DoubleEndedIterator<Item = &str> {
        self.undo.iter().map(|(l, _)| l.as_str())
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|(l, _)| l.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|(l, _)| l.as_str())
    }
}
