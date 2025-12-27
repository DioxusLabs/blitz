pub mod device;
pub mod preference;
pub mod process;
pub mod root;
use std::any::Any;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering as Ao};

pub(crate) use device::DeviceActor;
pub(crate) use preference::PreferenceActor;
pub(crate) use process::ProcessActor;
pub(crate) use root::RootActor;

use crate::{Connection, DevtoolsServer, GenericClientMessage, JsonValue, MessageWriter};

pub(crate) type ActorId = String;

pub(crate) fn generate_name(base: &str) -> String {
    static ACTOR_ID_COUNTER: AtomicUsize = AtomicUsize::new(0);
    let id = ACTOR_ID_COUNTER.fetch_add(1, Ao::Relaxed);
    format!("{base}-{id}")
}

// #[derive(Copy, Clone, PartialEq, Eq, Hash)]
// pub(crate) struct ActorId {
//     kind: &'static str,
//     id: Option<u32>,
// }

// impl Display for ActorId {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         match self.id {
//             Some(id) => write!(f, "{}-{}", self.kind, id),
//             None => write!(f, "{}", self.kind),
//         }
//     }
// }

// impl Display for ActorId {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         write!(f, "{}-{}", self.kind, self.id)
//     }
// }

// https://firefox-source-docs.mozilla.org/devtools/backend/protocol.html#error-packets
pub(crate) enum ActorMessageErr {
    NoSuchActor,
    UnrecognizedPacketType,
    MissingParameter,
    BadParameterType,
}

impl ActorMessageErr {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            ActorMessageErr::NoSuchActor => "noSuchActor",
            ActorMessageErr::UnrecognizedPacketType => "unrecognizedPacketType",
            ActorMessageErr::MissingParameter => "missingParameter",
            ActorMessageErr::BadParameterType => "badParameterType",
        }
    }
}

pub(crate) trait Actor: Any + Send + 'static {
    fn name(&self) -> String;

    fn handle_message<'a>(
        &self,
        ctx: &mut DevtoolContext<'a>,
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

    pub(crate) fn context(&mut self) -> DevtoolContext<'_> {
        DevtoolContext {
            writer: &mut self.writer,
            actors: &mut self.actors,
            actors_to_create: Vec::new(),
        }
    }

    pub(crate) fn handle_message(&mut self, msg: GenericClientMessage) {
        let mut ctx = self.context();
        let Some(actor) = ctx.actors.get(&msg.to) else {
            self.writer.write_err(msg.to, ActorMessageErr::NoSuchActor);
            return;
        };

        let result = actor.handle_message(&mut ctx, msg);
        if let Err(err) = result {
            ctx.write_err(actor.name(), err);
        }

        // Handle errors
        let actors_to_create = std::mem::take(&mut ctx.actors_to_create);
        for actor in actors_to_create {
            self.insert_actor(actor);
        }
    }
}

pub(crate) struct DevtoolContext<'a> {
    pub(crate) writer: &'a mut MessageWriter,
    pub(crate) actors: &'a HashMap<ActorId, Box<dyn Actor>>,
    pub(crate) actors_to_create: Vec<Box<dyn Actor>>,
}

impl DevtoolContext<'_> {
    fn push_actor(&mut self, actor: Box<dyn Actor>) {
        self.actors_to_create.push(actor);
    }

    fn write_msg(&mut self, from: String, data: JsonValue) {
        self.writer.write_msg(from, data);
    }
    fn write_err(&mut self, from: String, err: ActorMessageErr) {
        self.writer.write_err(from, err);
    }

    fn actor<T: Actor>(&self, name: &str) -> &T {
        (&*self.actors[name] as &dyn Any).downcast_ref().unwrap()
    }
}
