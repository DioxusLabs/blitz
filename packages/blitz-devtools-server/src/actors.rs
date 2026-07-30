pub mod css_properties;
pub mod device;
pub mod highlighter;
pub mod inspector;
pub mod layout;
pub mod page_style;
pub mod preference;
pub mod process;
pub mod root;
pub mod stubs;
pub mod tab;
pub mod walker;
pub mod watcher;

use std::any::Any;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering as Ao};

pub(crate) use device::DeviceActor;
pub(crate) use preference::PreferenceActor;
pub(crate) use process::ProcessActor;
pub(crate) use root::RootActor;

use crate::{Connection, DocumentProvider, GenericClientMessage, JsonValue, MessageWriter};

pub(crate) type ActorId = String;

pub(crate) fn generate_name(base: &str) -> String {
    static ACTOR_ID_COUNTER: AtomicUsize = AtomicUsize::new(0);
    let id = ACTOR_ID_COUNTER.fetch_add(1, Ao::Relaxed);
    format!("{base}-{id}")
}

// https://firefox-source-docs.mozilla.org/devtools/backend/protocol.html#error-packets
pub(crate) enum ActorMessageErr {
    NoSuchActor,
    UnrecognizedPacketType,
    MissingParameter,
    BadParameterType,
    NoSuchNode,
    NoSuchDocument,
}

impl ActorMessageErr {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            ActorMessageErr::NoSuchActor => "noSuchActor",
            ActorMessageErr::UnrecognizedPacketType => "unrecognizedPacketType",
            ActorMessageErr::MissingParameter => "missingParameter",
            ActorMessageErr::BadParameterType => "badParameterType",
            ActorMessageErr::NoSuchNode => "noSuchNode",
            ActorMessageErr::NoSuchDocument => "noSuchDocument",
        }
    }

    pub(crate) fn message(&self) -> &'static str {
        match self {
            ActorMessageErr::NoSuchActor => "No such actor",
            ActorMessageErr::UnrecognizedPacketType => "Unrecognized packet type",
            ActorMessageErr::MissingParameter => "Missing parameter",
            ActorMessageErr::BadParameterType => "Bad parameter type",
            ActorMessageErr::NoSuchNode => "No such node",
            ActorMessageErr::NoSuchDocument => "No such document",
        }
    }
}

pub(crate) trait Actor: Any + Send + 'static {
    fn name(&self) -> String;

    fn handle_message(
        &mut self,
        ctx: &mut DevtoolContext<'_>,
        message: GenericClientMessage,
    ) -> Result<(), ActorMessageErr>;
}

impl Connection {
    pub(crate) fn init(&mut self) {
        let pref = PreferenceActor::new();
        let device = DeviceActor::new();
        let process = ProcessActor::new();
        let root = RootActor::new(pref.name(), device.name(), process.name());
        self.insert_actor(Box::new(pref));
        self.insert_actor(Box::new(device));
        self.insert_actor(Box::new(process));
        self.insert_actor(Box::new(root));
    }

    pub(crate) fn insert_actor(&mut self, actor: Box<dyn Actor>) {
        self.actors.insert(actor.name(), actor);
    }

    pub(crate) fn handle_message(
        &mut self,
        msg: GenericClientMessage,
        docs: &mut dyn DocumentProvider,
    ) {
        // Temporarily remove the target actor from the map so that it can be
        // handled with `&mut self` while other actors remain accessible
        // through the context.
        let Some(mut actor) = self.actors.remove(&msg.to) else {
            // Firefox sends getUniqueSelector directly to node actors, which
            // are virtual (owned by the walker) rather than registered actors.
            // Reply with a placeholder selector rather than noSuchActor, which
            // would break the Rules and Flexbox panels.
            if msg.to.starts_with("node-") && msg.type_ == "getUniqueSelector" {
                self.writer
                    .write_msg(msg.to, serde_json::json!({ "value": "" }));
                return;
            }
            self.writer.write_err(msg.to, ActorMessageErr::NoSuchActor);
            return;
        };

        let mut ctx = DevtoolContext {
            writer: &mut self.writer,
            actors: &mut self.actors,
            actors_to_create: Vec::new(),
            docs,
        };

        let result = actor.handle_message(&mut ctx, msg);
        if let Err(err) = result {
            ctx.write_err(actor.name(), err);
        }

        let actors_to_create = std::mem::take(&mut ctx.actors_to_create);
        self.insert_actor(actor);
        for actor in actors_to_create {
            self.insert_actor(actor);
        }
    }

    pub(crate) fn notify_picker_event(
        &mut self,
        event: &crate::PickerEvent,
        docs: &mut dyn DocumentProvider,
    ) {
        use crate::PickerEvent;
        use crate::actors::walker::WalkerActor;

        let event_doc_id = match *event {
            PickerEvent::Hovered { doc_id, .. }
            | PickerEvent::Picked { doc_id, .. }
            | PickerEvent::Canceled { doc_id } => doc_id,
        };

        // Find this connection's walkers that are currently picking on the
        // document the event is for
        let walker_names: Vec<ActorId> = self
            .actors
            .values()
            .filter_map(|actor| {
                let any: &dyn Any = actor.as_ref();
                let walker = any.downcast_ref::<WalkerActor>()?;
                (walker.picking && walker.doc_id == event_doc_id).then(|| walker.name())
            })
            .collect();

        for walker_name in walker_names {
            let Some(mut actor) = self.actors.remove(&walker_name) else {
                continue;
            };
            {
                let any: &mut dyn Any = actor.as_mut();
                let walker = any.downcast_mut::<WalkerActor>().unwrap();
                let mut ctx = DevtoolContext {
                    writer: &mut self.writer,
                    actors: &mut self.actors,
                    actors_to_create: Vec::new(),
                    docs,
                };
                walker.handle_picker_event(&mut ctx, event);
            }
            self.insert_actor(actor);
        }
    }
}

pub(crate) struct DevtoolContext<'a> {
    pub(crate) writer: &'a mut MessageWriter,
    pub(crate) actors: &'a mut HashMap<ActorId, Box<dyn Actor>>,
    pub(crate) actors_to_create: Vec<Box<dyn Actor>>,
    pub(crate) docs: &'a mut dyn DocumentProvider,
}

impl DevtoolContext<'_> {
    /// Register a new actor with the connection. It will become routable once
    /// the current message has finished processing.
    pub(crate) fn push_actor(&mut self, actor: Box<dyn Actor>) {
        self.actors_to_create.push(actor);
    }

    pub(crate) fn write_msg(&mut self, from: String, data: JsonValue) {
        self.writer.write_msg(from, data);
    }
    pub(crate) fn write_err(&mut self, from: String, err: ActorMessageErr) {
        self.writer.write_err(from, err);
    }

    pub(crate) fn actor<T: Actor>(&self, name: &str) -> &T {
        (&*self.actors[name] as &dyn Any).downcast_ref().unwrap()
    }

    /// Run a callback with access to the document with the given id, returning
    /// `None` if no such document exists
    pub(crate) fn with_doc<R>(
        &mut self,
        doc_id: usize,
        cb: impl FnOnce(&mut blitz_dom::BaseDocument) -> R,
    ) -> Option<R> {
        let mut cb = Some(cb);
        let mut result = None;
        self.docs.with_document(doc_id, &mut |doc| {
            if let Some(cb) = cb.take() {
                result = Some(cb(doc));
            }
        });
        result
    }
}
