use blitz_dom::{local_name, BaseDocument};
use blitz_traits::{BlitzKeyEvent, BlitzMouseButtonEvent, Document, DomEvent, DomEventData};
use std::ops::{Deref, DerefMut};

// TODO: make generic
type D = BaseDocument;

pub struct Event<Doc: Document<Doc = D>> {
    doc: Doc,

    mouse_pos: (f32, f32),
    pub dom_mouse_pos: (f32, f32),
    mouse_down_node: Option<usize>,
}

impl<Doc: Document<Doc = D>> Deref for Event<Doc> {
    type Target = Doc;

    fn deref(&self) -> &Self::Target {
        &self.doc
    }
}

impl<Doc: Document<Doc = D>> DerefMut for Event<Doc> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.doc
    }
}

impl<Doc: Document<Doc = D>> Event<Doc> {
    pub fn new(doc: Doc) -> Self {
        Self {
            doc,
            mouse_pos: Default::default(),
            dom_mouse_pos: Default::default(),
            mouse_down_node: None,
        }
    }

    pub fn mouse_move(&mut self, x: f32, y: f32, zoom: f32) -> bool {
        let viewport_scroll = self.doc.as_ref().viewport_scroll();
        let dom_x = x + viewport_scroll.x as f32 / zoom;
        let dom_y = y + viewport_scroll.y as f32 / zoom;

        // println!("Mouse move: ({}, {})", x, y);
        // println!("Unscaled: ({}, {})",);

        self.mouse_pos = (x, y);
        self.dom_mouse_pos = (dom_x, dom_y);
        self.doc.as_mut().set_hover_to(dom_x, dom_y)
    }

    pub fn mouse_down(&mut self, event_data: BlitzMouseButtonEvent) {
        let Some(node_id) = self.doc.as_ref().get_hover_node_id() else {
            return;
        };
        let focussed_node_id = self.doc.as_ref().get_focussed_node_id();

        self.doc.as_mut().active_node();

        let chain = self.call_node_chain(node_id, DomEventData::MouseDown(event_data));

        if chain.is_some() {
            if let Some(focussed_node_id) = focussed_node_id {
                if node_id != focussed_node_id {
                    self.blur(focussed_node_id);
                }
            };

            let element = self.doc.as_ref().tree()[node_id].element_data().unwrap();
            let triggers_input_event = element.name.local == local_name!("input")
                || element.name.local == local_name!("textarea");

            if triggers_input_event {
                self.focus(node_id);
            }
        }

        self.mouse_down_node = Some(node_id);
    }

    pub fn mouse_up(&mut self, event_data: BlitzMouseButtonEvent, button: &str) {
        self.doc.as_mut().unactive_node();

        let Some(node_id) = self.doc.as_ref().get_hover_node_id() else {
            return;
        };

        self.call_node_chain(node_id, DomEventData::MouseUp(event_data.clone()));

        if self.mouse_down_node == Some(node_id) {
            self.click(event_data, node_id, button);
        } else if let Some(mouse_down_id) = self.mouse_down_node {
            // Anonymous node ids are unstable due to tree reconstruction. So we compare the id
            // of the first non-anonymous ancestor.
            if self.doc.as_ref().non_anon_ancestor_if_anon(mouse_down_id)
                == self.doc.as_ref().non_anon_ancestor_if_anon(node_id)
            {
                self.click(
                    event_data,
                    self.doc.as_ref().non_anon_ancestor_if_anon(node_id),
                    button,
                );
            }
        }
    }

    pub fn click(&mut self, event_data: BlitzMouseButtonEvent, node_id: usize, button: &str) {
        if button == "left" {
            let chain = self.call_node_chain(node_id, DomEventData::Click(event_data.clone()));

            if let Some(chain) = chain {
                let element = self.doc.as_ref().tree()[node_id].element_data().unwrap();
                let root_input = element.name.local == local_name!("input");

                for target in chain.iter() {
                    let element = self.doc.as_ref().tree()[*target].element_data().unwrap();

                    let trigger_label = element.name.local == *"label";
                    let triggers_input_event = element.name.local == local_name!("input")
                        && matches!(
                            element.attr(local_name!("type")),
                            Some("checkbox") | Some("radio")
                        );

                    if triggers_input_event {
                        self.input(None);
                    } else if trigger_label && !root_input {
                        if let Some(input_id) = self.label_bound_input_element(*target) {
                            self.click(event_data.clone(), input_id, "left");
                        }
                    }
                }
            }
        }
    }

    pub fn key_down(&mut self, event_data: BlitzKeyEvent, node_id: usize) {
        let chain = self.call_node_chain(node_id, DomEventData::KeyDown(event_data.clone()));

        if let Some(chain) = chain {
            for target in chain.iter() {
                let element = self.doc.as_ref().tree()[*target].element_data().unwrap();

                let triggers_input_event = element.name.local == local_name!("input")
                    && matches!(
                        element.attr(local_name!("type")),
                        None | Some("text" | "password" | "email" | "search")
                    );

                if triggers_input_event {
                    self.key_press(event_data.clone(), node_id);
                }
            }
        }
    }

    pub fn key_press(&mut self, event_data: BlitzKeyEvent, node_id: usize) {
        let chain = self.call_node_chain(node_id, DomEventData::KeyPress(event_data.clone()));

        if let Some(chain) = chain {
            for target in chain.iter() {
                let element = self.doc.as_ref().tree()[*target].element_data().unwrap();

                let triggers_input_event = element.name.local == local_name!("input")
                    && matches!(
                        element.attr(local_name!("type")),
                        None | Some("text" | "password" | "email" | "search")
                    );

                if triggers_input_event {
                    self.input(Some(event_data.clone()));
                }
            }
        }
    }

    pub fn key_up(&mut self, event_data: BlitzKeyEvent, node_id: usize) {
        self.call_node_chain(node_id, DomEventData::KeyUp(event_data));
    }

    pub fn input(&mut self, event_data: Option<BlitzKeyEvent>) {
        let Some(node_id) = self.doc.as_ref().get_focussed_node_id() else {
            return;
        };

        if let Some(event_data) = event_data {
            self.call_node_chain(node_id, DomEventData::Input(event_data));
        } else {
            self.call_node_chain(node_id, DomEventData::Event("input"));
        }
    }

    pub fn focus(&mut self, node_id: usize) {
        self.call_node_chain(node_id, DomEventData::Focus);
    }

    pub fn blur(&mut self, node_id: usize) {
        self.call_node_chain(node_id, DomEventData::Blur);
    }

    fn label_bound_input_element(&self, label_node_id: usize) -> Option<usize> {
        let bound_input_elements = self.doc.as_ref().label_bound_input_elements(label_node_id);

        // Find the first node that is not uninstalled.
        let root_node_id = self.doc.as_ref().root_node().id;
        bound_input_elements.into_iter().find_map(|n| {
            let mut next_node_id = n.id;
            loop {
                let node = &self.doc.as_ref().tree()[next_node_id];
                if let Some(node_id) = node.parent {
                    next_node_id = node_id;
                } else {
                    break;
                }
            }
            if next_node_id == root_node_id {
                Some(n.id)
            } else {
                None
            }
        })
    }

    fn call_node_chain(&mut self, target: usize, event_data: DomEventData) -> Option<Vec<usize>> {
        let node_data = &self.doc.as_ref().tree()[target].raw_dom_data;
        if node_data.is_element_with_tag_name(&local_name!("input"))
            && node_data.attr(local_name!("disabled")).is_some()
        {
            return None;
        }

        // Collect the nodes into a chain by traversing upwards
        // This is important so the "capture" phase can be implemented
        let chain = self.node_chain(target);
        let mut event = DomEvent::new(target, event_data, chain.clone());

        for target in chain.iter() {
            event.current_target = Some(*target);
            self.doc.handle_event(&mut event);
            if !event.default_prevented {
                // Default event
                self.doc.as_mut().handle_event(&mut event);
            }
            event.current_target = None;

            if !event.bubbles || event.stop_propagation {
                break;
            }
        }

        if !event.default_prevented {
            Some(chain)
        } else {
            None
        }
    }

    /// Collect the nodes into a chain by traversing upwards
    fn node_chain(&self, node_id: usize) -> Vec<usize> {
        let mut next_node_id = Some(node_id);
        let mut chain = Vec::with_capacity(16);

        while let Some(node_id) = next_node_id {
            let node = &self.doc.as_ref().tree()[node_id];

            if node.is_element() {
                chain.push(node_id);
            }

            next_node_id = node.parent;
        }

        chain
    }
}
