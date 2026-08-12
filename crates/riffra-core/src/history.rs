use crate::session::CreativeSession;

const HISTORY_LIMIT: usize = 40;

#[derive(Default)]
pub(crate) struct History {
    undo: Vec<CreativeSession>,
    redo: Vec<CreativeSession>,
}

impl History {
    pub(crate) fn record(&mut self, previous: CreativeSession) {
        self.undo.push(previous);
        if self.undo.len() > HISTORY_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub(crate) fn take_undo(&mut self) -> Option<CreativeSession> {
        self.undo.pop()
    }

    pub(crate) fn take_redo(&mut self) -> Option<CreativeSession> {
        self.redo.pop()
    }

    pub(crate) fn push_redo(&mut self, session: CreativeSession) {
        self.redo.push(session);
        if self.redo.len() > HISTORY_LIMIT {
            self.redo.remove(0);
        }
    }

    pub(crate) fn push_undo(&mut self, session: CreativeSession) {
        self.undo.push(session);
        if self.undo.len() > HISTORY_LIMIT {
            self.undo.remove(0);
        }
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}
