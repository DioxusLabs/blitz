use crate::layout::damage::ALL_DAMAGE;
use crate::mutator::DocumentMutator;
use crate::node::{SelectData, SelectOptionSnapshot, SpecialElementData};
use crate::{BaseDocument, local_name};
use blitz_traits::node_id::NodeId;

impl BaseDocument {
    pub fn select_options(&self, select: NodeId) -> Vec<NodeId> {
        let mut options = Vec::new();
        for &child in &self.nodes[select].children {
            if self.nodes[child]
                .data
                .is_element_with_tag_name(&local_name!("option"))
            {
                options.push(child);
            } else if self.nodes[child]
                .data
                .is_element_with_tag_name(&local_name!("optgroup"))
            {
                options.extend(self.nodes[child].children.iter().copied().filter(|&id| {
                    self.nodes[id]
                        .data
                        .is_element_with_tag_name(&local_name!("option"))
                }));
            }
        }
        options
    }

    pub fn selected_option(&self, select: NodeId) -> Option<NodeId> {
        match &self.get_node(select)?.element_data()?.special_data {
            SpecialElementData::Select(data) => data.selected,
            _ => None,
        }
    }

    pub fn option_label(&self, option: NodeId) -> String {
        self.nodes[option]
            .attr(local_name!("label"))
            .map(str::to_owned)
            .unwrap_or_else(|| self.option_text(option))
    }

    pub fn option_text(&self, option: NodeId) -> String {
        self.nodes[option]
            .text_content()
            .split_ascii_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn option_value(&self, option: NodeId) -> String {
        self.nodes[option]
            .attr(local_name!("value"))
            .map(str::to_owned)
            .unwrap_or_else(|| self.option_text(option))
    }

    pub fn option_disabled(&self, option: NodeId) -> bool {
        let node = &self.nodes[option];
        node.attr(local_name!("disabled")).is_some()
            || node.parent.is_some_and(|parent| {
                self.nodes[parent]
                    .data
                    .is_element_with_tag_name(&local_name!("optgroup"))
                    && self.nodes[parent].attr(local_name!("disabled")).is_some()
            })
    }

    pub fn set_selected_option(&mut self, select: NodeId, option: Option<NodeId>) -> bool {
        if self.get_node(select).is_none() {
            return false;
        }
        if option.is_some_and(|id| !self.select_options(select).contains(&id)) {
            return false;
        }
        let label = option.map(|id| self.option_label(id)).unwrap_or_default();
        let Some(element) = self.nodes[select].element_data_mut() else {
            return false;
        };
        let SpecialElementData::Select(data) = &mut element.special_data else {
            return false;
        };
        data.selected = option;
        data.label = label;
        self.nodes[select].insert_damage(ALL_DAMAGE);
        self.nodes[select].mark_ancestors_dirty();
        self.shell_provider.request_redraw();
        true
    }
}

impl DocumentMutator<'_> {
    pub(crate) fn mark_select_dirty(&mut self, node_id: NodeId) {
        let mut ancestor = Some(node_id);
        while let Some(id) = ancestor {
            if self.doc.select_nodes.contains(&id) {
                self.doc.dirty_select_nodes.insert(id);
                break;
            }
            ancestor = self.doc.nodes[id].parent;
        }
    }

    pub(crate) fn refresh_selects(&mut self) {
        let selects = std::mem::take(&mut self.doc.dirty_select_nodes);
        for id in selects {
            if !self.doc.select_nodes.contains(&id) {
                continue;
            }
            let node = &self.doc.nodes[id];
            if node.attr(local_name!("disabled")).is_some() && self.doc.open_select == Some(id) {
                self.doc.close_select();
            }
            let node = &self.doc.nodes[id];
            let supported = node.element_data().unwrap().name.ns == markup5ever::ns!(html)
                && node.attr(local_name!("multiple")).is_none()
                && node
                    .attr(local_name!("size"))
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(1)
                    <= 1;
            if !supported {
                if self.doc.open_select == Some(id) {
                    self.doc.close_select();
                }
                if matches!(
                    &self.doc.nodes[id].element_data().unwrap().special_data,
                    SpecialElementData::Select(_)
                ) {
                    self.doc.nodes[id].element_data_mut().unwrap().special_data =
                        SpecialElementData::None;
                    self.doc.nodes[id].insert_damage(ALL_DAMAGE);
                }
                self.doc.dirty_select_nodes.remove(&id);
                continue;
            }
            let value = node.attr(local_name!("value")).map(str::to_owned);
            let options: Vec<_> = self
                .doc
                .select_options(id)
                .into_iter()
                .map(|option| SelectOptionSnapshot {
                    node_id: option,
                    selected: self.doc.nodes[option]
                        .attr(local_name!("selected"))
                        .is_some(),
                    disabled: self.doc.option_disabled(option),
                    value: self.doc.option_value(option),
                })
                .collect();
            let previous = &node.element_data().unwrap().special_data;
            let changed = !matches!(previous, SpecialElementData::Select(data)
            if data.value == value && data.options.len() == options.len()
                && data.options.iter().zip(&options).all(|(old, new)| {
                    old.node_id == new.node_id && old.selected == new.selected
                        && old.disabled == new.disabled
                        && (value.is_none() || old.value == new.value)
                }));
            let selected = if changed {
                if let Some(value) = &value {
                    options
                        .iter()
                        .find(|option| option.value == *value)
                        .map(|option| option.node_id)
                } else {
                    options
                        .iter()
                        .rev()
                        .find(|option| option.selected)
                        .or_else(|| options.iter().find(|option| !option.disabled))
                        .map(|option| option.node_id)
                }
            } else {
                self.doc.selected_option(id)
            };
            let label = selected
                .map(|option| self.doc.option_label(option))
                .unwrap_or_default();
            let label_changed =
                !matches!(previous, SpecialElementData::Select(data) if data.label == label);
            let open = matches!(
                &self.doc.nodes[id].element_data().unwrap().special_data,
                SpecialElementData::Select(SelectData { open: true, .. })
            );
            let (highlighted, scroll_offset) =
                match &self.doc.nodes[id].element_data().unwrap().special_data {
                    SpecialElementData::Select(data) => (
                        data.highlighted.filter(|option| {
                            options.iter().any(|o| o.node_id == *option && !o.disabled)
                        }),
                        data.scroll_offset,
                    ),
                    _ => (None, 0),
                };
            self.doc.nodes[id].element_data_mut().unwrap().special_data =
                SpecialElementData::Select(SelectData {
                    selected,
                    open,
                    highlighted,
                    scroll_offset,
                    label,
                    options,
                    value,
                });
            if label_changed {
                self.doc.nodes[id].insert_damage(ALL_DAMAGE);
                self.doc.nodes[id].mark_ancestors_dirty();
            }
            self.doc.dirty_select_nodes.remove(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DocumentConfig, qual_name};

    fn fixture() -> (BaseDocument, NodeId, NodeId, NodeId) {
        let mut doc = BaseDocument::new(DocumentConfig::default());
        let root = doc.root_node_id;
        let (select, first, second);
        {
            let mut m = doc.mutate();
            select = m.create_element(qual_name!("select", html), vec![]);
            first = m.create_element(qual_name!("option", html), vec![]);
            second = m.create_element(qual_name!("option", html), vec![]);
            let text = m.create_text_node("  Second\n label  ");
            m.append_children(second, &[text]);
            m.append_children(select, &[first, second]);
            m.append_children(root, &[select]);
        }
        (doc, select, first, second)
    }

    #[test]
    fn select_initial_selectedness() {
        let mut doc = BaseDocument::new(DocumentConfig::default());
        let root = doc.root_node_id;
        let (select, first, last);
        {
            let mut m = doc.mutate();
            select = m.create_element(qual_name!("select", html), vec![]);
            first = m.create_element(qual_name!("option", html), vec![]);
            last = m.create_element(qual_name!("option", html), vec![]);
            m.set_attribute(first, qual_name!("selected"), "");
            m.set_attribute(last, qual_name!("selected"), "");
            m.append_children(select, &[first, last]);
            m.append_children(root, &[select]);
        }
        assert_eq!(doc.selected_option(select), Some(last));
        {
            let mut m = doc.mutate();
            m.clear_attribute(first, qual_name!("selected"));
            m.clear_attribute(last, qual_name!("selected"));
            m.set_attribute(first, qual_name!("disabled"), "");
            m.set_attribute(last, qual_name!("label"), "Displayed label");
        }
        assert_eq!(doc.selected_option(select), Some(last));
        assert_eq!(doc.option_label(last), "Displayed label");
        let SpecialElementData::Select(data) =
            &doc.nodes[select].element_data().unwrap().special_data
        else {
            panic!()
        };
        assert_eq!(data.label, "Displayed label");
    }

    #[test]
    fn select_initial_and_attribute_selection() {
        let (mut doc, select, first, second) = fixture();
        assert_eq!(doc.selected_option(select), Some(first));
        doc.mutate()
            .set_attribute(first, qual_name!("selected"), "");
        doc.mutate()
            .set_attribute(second, qual_name!("selected"), "");
        assert_eq!(doc.selected_option(select), Some(second));
        assert_eq!(doc.option_label(second), "Second label");
        doc.mutate().clear_attribute(second, qual_name!("selected"));
        assert_eq!(doc.selected_option(select), Some(first));
        doc.mutate()
            .set_attribute(select, qual_name!("value"), "Second label");
        assert_eq!(doc.selected_option(select), Some(second));
        doc.mutate()
            .set_attribute(select, qual_name!("value"), "missing");
        assert_eq!(doc.selected_option(select), None);
    }

    #[test]
    fn select_disabled_optgroup_and_removal() {
        let (mut doc, select, first, second) = fixture();
        let group;
        {
            let mut m = doc.mutate();
            group = m.create_element(qual_name!("optgroup", html), vec![]);
            m.append_children(group, &[first]);
            m.insert_nodes_before(second, &[group]);
            m.set_attribute(group, qual_name!("disabled"), "");
        }
        assert_eq!(doc.selected_option(select), Some(second));
        doc.mutate().remove_and_drop_node(second);
        assert_eq!(doc.selected_option(select), None);
        doc.mutate().clear_attribute(group, qual_name!("disabled"));
        assert_eq!(doc.selected_option(select), Some(first));
        assert!(!doc.set_selected_option(select, Some(group)));
        assert!(doc.set_selected_option(select, None));
        assert_eq!(doc.selected_option(select), None);
    }

    #[test]
    fn select_empty_and_unsupported() {
        let (mut doc, select, first, second) = fixture();
        doc.mutate()
            .set_attribute(select, qual_name!("multiple"), "");
        assert!(!matches!(
            doc.nodes[select].element_data().unwrap().special_data,
            SpecialElementData::Select(_)
        ));
        doc.mutate().clear_attribute(select, qual_name!("multiple"));
        doc.mutate().remove_and_drop_node(first);
        doc.mutate().remove_and_drop_node(second);
        assert_eq!(doc.selected_option(select), None);
        doc.mutate().set_attribute(select, qual_name!("size"), "2");
        assert!(!matches!(
            doc.nodes[select].element_data().unwrap().special_data,
            SpecialElementData::Select(_)
        ));
    }

    #[test]
    fn select_dirty_tracking_and_registration() {
        let (mut doc, select, _, second) = fixture();
        assert!(doc.select_nodes.contains(&select));
        assert!(doc.dirty_select_nodes.is_empty());
        let root = doc.root_node_id;
        {
            let mut m = doc.mutate();
            let unrelated = m.create_element(qual_name!("div", html), vec![]);
            m.append_children(root, &[unrelated]);
            m.set_attribute(unrelated, qual_name!("title"), "unrelated");
            assert!(m.doc.dirty_select_nodes.is_empty());
            m.set_attribute(second, qual_name!("label"), "Updated");
            assert_eq!(m.doc.dirty_select_nodes.len(), 1);
            assert!(m.doc.dirty_select_nodes.contains(&select));
        }
        assert!(doc.dirty_select_nodes.is_empty());
        doc.mutate().remove_node(select);
        assert!(!doc.select_nodes.contains(&select));
        assert!(doc.dirty_select_nodes.is_empty());
        doc.mutate().append_children(root, &[select]);
        assert!(doc.select_nodes.contains(&select));
        doc.mutate().remove_and_drop_node(select);
        assert!(!doc.select_nodes.contains(&select));
        assert!(doc.dirty_select_nodes.is_empty());
    }

    #[test]
    fn select_label_is_not_a_dom_child() {
        let (mut doc, select, first, second) = fixture();
        assert_eq!(&*doc.nodes[select].children, &[first, second]);
        assert!(doc.set_selected_option(select, Some(second)));
        assert_eq!(&*doc.nodes[select].children, &[first, second]);
        assert!(!doc.set_selected_option(select, Some(doc.root_node_id)));
        assert_eq!(doc.selected_option(select), Some(second));
        assert!(doc.set_selected_option(select, None));
        let SpecialElementData::Select(data) =
            &doc.nodes[select].element_data().unwrap().special_data
        else {
            panic!()
        };
        assert!(data.label.is_empty());
        assert_eq!(&*doc.nodes[select].children, &[first, second]);
    }

    #[test]
    fn select_non_html_namespace_is_not_registered() {
        let mut doc = BaseDocument::new(DocumentConfig::default());
        let root = doc.root_node_id;
        let select;
        {
            let mut m = doc.mutate();
            select = m.create_element(qual_name!("select", svg), vec![]);
            m.append_children(root, &[select]);
            m.set_attribute(select, qual_name!("value"), "value");
        }
        assert!(!doc.select_nodes.contains(&select));
        assert!(matches!(
            doc.nodes[select].element_data().unwrap().special_data,
            SpecialElementData::None
        ));
        assert!(!doc.open_select(select));
        assert!(!doc.set_selected_option(select, None));
    }

    #[test]
    fn select_option_text_mutations_update_label() {
        let (mut doc, select, _, second) = fixture();
        assert!(doc.set_selected_option(select, Some(second)));
        let text = doc.nodes[second].children[0];
        {
            let mut m = doc.mutate();
            m.set_node_text(text, "Changed");
            assert!(m.doc.dirty_select_nodes.contains(&select));
        }
        doc.mutate().append_text_to_node(text, " text").unwrap();
        let SpecialElementData::Select(data) =
            &doc.nodes[select].element_data().unwrap().special_data
        else {
            panic!()
        };
        assert_eq!(data.label, "Changed text");
        assert!(doc.dirty_select_nodes.is_empty());
    }
}
