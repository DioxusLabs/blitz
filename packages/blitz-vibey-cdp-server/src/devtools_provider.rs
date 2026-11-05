//! Live DOM mutation events. A [`DevtoolsProvider`] installed on each
//! inspected document buffers the mutations reported by Blitz's
//! `DocumentMutator`; the server later translates the buffered records into
//! `DOM` domain events against the document's current state.

use std::sync::{Arc, Mutex};

use blitz_traits::devtools::DevtoolsProvider;
use blitz_traits::node_id::NodeId;

use crate::DevtoolsWaker;

/// A DOM mutation, as reported to a [`DevtoolsProvider`]
pub(crate) enum MutationRecord {
    Inserted { parent_id: NodeId, node_id: NodeId },
    Removed { parent_id: NodeId, node_id: NodeId },
    AttributeChanged { node_id: NodeId, name: String },
    CharacterDataChanged { node_id: NodeId },
}

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

    fn push(&self, doc_id: usize, record: MutationRecord) {
        let mut records = self.records.lock().unwrap();
        let was_empty = records.is_empty();
        records.push((doc_id, record));
        drop(records);
        // Wake the embedder once per batch, so that the records are processed
        // once the current document update has finished
        if was_empty {
            self.waker.wake();
        }
    }
}

struct DocumentRecorder {
    doc_id: usize,
    log: Arc<MutationLog>,
}

impl DevtoolsProvider for DocumentRecorder {
    fn node_inserted(&self, parent_id: NodeId, node_id: NodeId) {
        self.log
            .push(self.doc_id, MutationRecord::Inserted { parent_id, node_id });
    }

    fn node_removed(&self, parent_id: NodeId, node_id: NodeId) {
        self.log
            .push(self.doc_id, MutationRecord::Removed { parent_id, node_id });
    }

    fn attribute_changed(&self, node_id: NodeId, name: &str) {
        let name = name.to_string();
        self.log.push(
            self.doc_id,
            MutationRecord::AttributeChanged { node_id, name },
        );
    }

    fn character_data_changed(&self, node_id: NodeId) {
        self.log.push(
            self.doc_id,
            MutationRecord::CharacterDataChanged { node_id },
        );
    }
}
