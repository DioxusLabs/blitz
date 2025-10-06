use std::ops::{Deref, DerefMut};

use markup5ever::QualName;

/// A tag attribute, e.g. `class="test"` in `<div class="test" ...>`.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Debug)]
pub struct Attribute {
    /// The name of the attribute (e.g. the `class` in `<div class="test">`)
    pub name: QualName,
    /// The value of the attribute (e.g. the `"test"` in `<div class="test">`)
    pub value: String,
}

#[derive(Clone, Debug)]
pub struct Attributes {
    inner: Vec<Attribute>,
}

impl Attributes {
    pub fn new(inner: Vec<Attribute>) -> Self {
        Self { inner }
    }

    pub fn set(&mut self, name: QualName, value: &str) {
        let existing_attr = self.inner.iter_mut().find(|a| a.name == name);
        if let Some(existing_attr) = existing_attr {
            existing_attr.value.clear();
            existing_attr.value.push_str(value);
        } else {
            self.push(Attribute {
                name: name.clone(),
                value: value.to_string(),
            });
        }
    }

    pub fn remove(&mut self, name: &QualName) -> Option<Attribute> {
        let idx = self.inner.iter().position(|attr| attr.name == *name);
        idx.map(|idx| self.inner.remove(idx))
    }
}

impl Deref for Attributes {
    type Target = Vec<Attribute>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl DerefMut for Attributes {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
