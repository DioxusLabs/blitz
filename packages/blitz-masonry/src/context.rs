use std::{cell::RefCell, rc::Rc};
use taffy::{NodeId, TaffyTree};

#[derive(Default)]
pub struct Inner {
    pub taffy: TaffyTree,
    pub font_size: f32,
    pub parent_layout_id: Option<NodeId>,
}

#[derive(Clone, Default)]
pub struct Context {
    pub inner: Rc<RefCell<Inner>>,
}

impl Context {
    pub fn current() -> Option<Self> {
        CONTEXT.try_with(|cell| cell.borrow().clone()).unwrap()
    }

    pub fn enter(self) -> Option<Self> {
        CONTEXT.try_with(|cell| cell.replace(Some(self))).unwrap()
    }
}

thread_local! {
    static CONTEXT: RefCell<Option<Context>> = RefCell::new(None);
}
