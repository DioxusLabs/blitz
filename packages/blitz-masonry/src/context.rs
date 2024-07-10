use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Debug, Default)]
pub struct Inner {
    pub font_size: f32,
}

#[derive(Clone, Debug, Default)]
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
