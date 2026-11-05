//! Live DOM mutation events. A [`DevtoolsProvider`] installed on each
//! inspected document buffers the batches of mutations reported by Blitz's
//! `DocumentMutator`; the server later translates the buffered records into
//! `DOM` domain events against the document's current state.

use std::sync::{Arc, Mutex};

use blitz_traits::devtools::{DevtoolsProvider, MutationRecord};

use crate::DevtoolsWaker;

/// Mutation records buffered for all inspected documents (keyed by document id)
pub(crate) struct MutationLog {
    records: Mutex<Vec<(usize, MutationRecord)>>,
    waker: Arc<dyn DevtoolsWaker>,
}

impl MutationLog {
    pub(crate) fn new(waker: Arc<dyn DevtoolsWaker>) -> Self {
        MutationLog {
            records: Mutex::new(Vec::new()),
            waker,
        }
    }

    /// A provider recording the mutations of the document with the given id
    pub(crate) fn recorder(self: &Arc<Self>, doc_id: usize) -> Arc<dyn DevtoolsProvider> {
        Arc::new(DocumentRecorder {
            doc_id,
            log: Arc::clone(self),
        })
    }

    /// Take all buffered records
    pub(crate) fn take(&self) -> Vec<(usize, MutationRecord)> {
        std::mem::take(&mut *self.records.lock().unwrap())
    }
}

struct DocumentRecorder {
    doc_id: usize,
    log: Arc<MutationLog>,
}

impl DevtoolsProvider for DocumentRecorder {
    fn mutations(&self, records: &[MutationRecord]) {
        let mut buffered = self.log.records.lock().unwrap();
        let was_empty = buffered.is_empty();
        buffered.extend(records.iter().map(|record| (self.doc_id, record.clone())));
        drop(buffered);
        // Wake the embedder once per batch, so that the records are processed
        // once the current document update has finished
        if was_empty {
            self.log.waker.wake();
        }
    }
}
