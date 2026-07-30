//! Minimal stub actors that exist so that devtools clients can complete
//! their initialization sequence. They accept configuration/lifecycle
//! messages and reply with empty (or empty-list) responses.

use serde_json::json;

use crate::GenericClientMessage;
use crate::actors::{Actor, ActorId, ActorMessageErr, DevtoolContext, generate_name};

/// A generic stub actor which replies with an empty response to any message.
/// Used for configuration actors (which receive `updateConfiguration`
/// messages), the frame target, and other actors which need to exist but
/// whose behavior is not (yet) implemented.
pub(crate) struct StubActor {
    name: String,
}

impl StubActor {
    pub(crate) fn new(base: &str) -> Self {
        Self {
            name: generate_name(base),
        }
    }
}

impl Actor for StubActor {
    fn name(&self) -> ActorId {
        self.name.clone()
    }

    fn handle_message(
        &mut self,
        ctx: &mut DevtoolContext<'_>,
        message: GenericClientMessage,
    ) -> Result<(), ActorMessageErr> {
        match &*message.type_ {
            "listFrames" => {
                ctx.write_msg(self.name(), json!({ "frames": [] }));
            }
            "listWorkers" => {
                ctx.write_msg(self.name(), json!({ "workers": [] }));
            }
            _ => {
                ctx.write_msg(self.name(), json!({}));
            }
        }
        Ok(())
    }
}

/// Stub console actor: reports no cached messages and no listeners
pub(crate) struct ConsoleActor {
    name: String,
}

impl ConsoleActor {
    pub(crate) fn new() -> Self {
        Self {
            name: generate_name("console"),
        }
    }
}

impl Actor for ConsoleActor {
    fn name(&self) -> ActorId {
        self.name.clone()
    }

    fn handle_message(
        &mut self,
        ctx: &mut DevtoolContext<'_>,
        message: GenericClientMessage,
    ) -> Result<(), ActorMessageErr> {
        match &*message.type_ {
            "startListeners" => {
                ctx.write_msg(self.name(), json!({ "startedListeners": [] }));
            }
            "stopListeners" => {
                ctx.write_msg(self.name(), json!({ "stoppedListeners": [] }));
            }
            "getCachedMessages" => {
                ctx.write_msg(self.name(), json!({ "messages": [] }));
            }
            "autocomplete" => {
                ctx.write_msg(self.name(), json!({ "matches": [], "matchProp": "" }));
            }
            _ => {
                ctx.write_msg(self.name(), json!({}));
            }
        }
        Ok(())
    }
}

/// Stub thread actor: accepts attach/reconfigure and reports no sources
pub(crate) struct ThreadActor {
    name: String,
}

impl ThreadActor {
    pub(crate) fn new() -> Self {
        Self {
            name: generate_name("thread"),
        }
    }
}

impl Actor for ThreadActor {
    fn name(&self) -> ActorId {
        self.name.clone()
    }

    fn handle_message(
        &mut self,
        ctx: &mut DevtoolContext<'_>,
        message: GenericClientMessage,
    ) -> Result<(), ActorMessageErr> {
        match &*message.type_ {
            "sources" => {
                ctx.write_msg(self.name(), json!({ "sources": [] }));
            }
            _ => {
                ctx.write_msg(self.name(), json!({}));
            }
        }
        Ok(())
    }
}
