//! Integration tests for Shadow DOM and Custom Element support.
//!
//! These only compile/run with the `shadow-dom` feature enabled:
//!   cargo test -p blitz-dom --features shadow-dom

#![cfg(feature = "shadow-dom")]

use std::cell::RefCell;
use std::rc::Rc;

use blitz_dom::node::{CustomElement, CustomElementCtx, CustomElementDefinition};
use blitz_dom::{BaseDocument, DocumentConfig, LocalName, QualName, ShadowRootMode, ns};

fn qname(local: &str) -> QualName {
    QualName {
        prefix: None,
        ns: ns!(html),
        local: LocalName::from(local),
    }
}

/// Build a document with `<html><body>` and return (doc, body_id).
fn doc_with_body() -> (BaseDocument, usize) {
    let mut doc = BaseDocument::new(DocumentConfig::default());
    let mut mutator = doc.mutate();
    let html = mutator.create_element(qname("html"), Vec::new());
    let body = mutator.create_element(qname("body"), Vec::new());
    mutator.append_children(html, &[body]);
    mutator.append_children(0, &[html]);
    drop(mutator);
    (doc, body)
}

#[test]
fn attach_shadow_creates_shadow_root() {
    let (mut doc, body) = doc_with_body();
    let mut mutator = doc.mutate();
    let host = mutator.create_element(qname("my-host"), Vec::new());
    mutator.append_children(body, &[host]);
    let shadow_root = mutator.attach_shadow(host, ShadowRootMode::Open);
    drop(mutator);

    assert_eq!(doc.shadow_root_id(host), Some(shadow_root));
    assert!(doc.get_node(shadow_root).unwrap().is_shadow_root());
    assert_eq!(doc.get_node(shadow_root).unwrap().parent, Some(host));
}

#[test]
fn shadow_content_is_styled_and_laid_out() {
    let (mut doc, body) = doc_with_body();
    let mut mutator = doc.mutate();

    let host = mutator.create_element(qname("my-host"), Vec::new());
    mutator.set_attribute(host, qname("style"), "display:block");
    mutator.append_children(body, &[host]);

    let shadow_root = mutator.attach_shadow(host, ShadowRootMode::Open);
    let div = mutator.create_element(qname("div"), Vec::new());
    mutator.set_attribute(div, qname("style"), "width:50px;height:30px");
    mutator.append_children(shadow_root, &[div]);
    drop(mutator);

    doc.resolve(0.0);

    // The shadow div should have been styled (Stylo traversed the flattened
    // tree) and laid out at its specified size.
    let div_node = doc.get_node(div).unwrap();
    assert!(
        div_node.primary_styles().is_some(),
        "shadow content should be styled"
    );
    assert_eq!(div_node.final_layout.size.width, 50.0);
    assert_eq!(div_node.final_layout.size.height, 30.0);
}

#[test]
fn slot_projects_light_dom_children() {
    let (mut doc, body) = doc_with_body();
    let mut mutator = doc.mutate();

    let host = mutator.create_element(qname("my-host"), Vec::new());
    mutator.set_attribute(host, qname("style"), "display:block");
    // Light DOM child
    let light = mutator.create_element(qname("span"), Vec::new());
    mutator.set_attribute(
        light,
        qname("style"),
        "display:block;width:40px;height:20px",
    );
    mutator.append_children(host, &[light]);
    mutator.append_children(body, &[host]);

    // Shadow tree: a wrapper containing a default <slot>
    let shadow_root = mutator.attach_shadow(host, ShadowRootMode::Open);
    let wrapper = mutator.create_element(qname("div"), Vec::new());
    mutator.set_attribute(wrapper, qname("style"), "display:block");
    let slot = mutator.create_element(qname("slot"), Vec::new());
    mutator.append_children(wrapper, &[slot]);
    mutator.append_children(shadow_root, &[wrapper]);
    drop(mutator);

    doc.resolve(0.0);

    // The light-DOM span should be assigned to the slot and laid out.
    let light_node = doc.get_node(light).unwrap();
    assert_eq!(
        light_node.element_data().unwrap().assigned_slot,
        Some(slot),
        "light child should be assigned to the default slot"
    );
    assert!(light_node.primary_styles().is_some());
    assert_eq!(light_node.final_layout.size.width, 40.0);
    assert_eq!(light_node.final_layout.size.height, 20.0);
}

struct GreetWidget {
    log: Rc<RefCell<Vec<String>>>,
}

impl CustomElement for GreetWidget {
    fn connected(&mut self, ctx: &mut CustomElementCtx<'_, '_>) {
        self.log.borrow_mut().push("connected".to_string());
        let name = ctx.host_attr(LocalName::from("name")).unwrap_or_default();
        // Build the shadow tree via the mutator API (the default test config
        // has no HTML parser, so we can't use set_shadow_html here).
        let shadow_root = ctx.shadow_root_id();
        let div = ctx.mutator().create_element(qname("div"), Vec::new());
        let text = ctx.mutator().create_text_node(&format!("Hello {name}"));
        ctx.mutator().append_children(div, &[text]);
        ctx.mutator().append_children(shadow_root, &[div]);
    }

    fn disconnected(&mut self, _ctx: &mut CustomElementCtx<'_, '_>) {
        self.log.borrow_mut().push("disconnected".to_string());
    }

    fn attribute_changed(
        &mut self,
        _ctx: &mut CustomElementCtx<'_, '_>,
        name: &str,
        _old: Option<&str>,
        new: Option<&str>,
    ) {
        self.log
            .borrow_mut()
            .push(format!("attr:{name}={}", new.unwrap_or("")));
    }
}

#[test]
fn custom_element_registry_upgrades_on_insertion() {
    let (mut doc, body) = doc_with_body();

    let log = Rc::new(RefCell::new(Vec::new()));
    let log_clone = log.clone();
    doc.define_custom_element(
        LocalName::from("greet-box"),
        CustomElementDefinition::new(move || {
            Box::new(GreetWidget {
                log: log_clone.clone(),
            })
        }),
    );

    let mut mutator = doc.mutate();
    let host = mutator.create_element(qname("greet-box"), Vec::new());
    mutator.set_attribute(host, qname("name"), "World");
    mutator.append_children(body, &[host]);
    drop(mutator);

    // The element should have been upgraded: a shadow root attached and
    // `connected` run.
    assert!(doc.shadow_root_id(host).is_some(), "shadow root attached");
    assert!(
        log.borrow().iter().any(|s| s == "connected"),
        "connected callback ran"
    );

    doc.resolve(0.0);

    // The shadow content built by the controller should be styled.
    let shadow_root = doc.shadow_root_id(host).unwrap();
    let shadow_div = doc.get_node(shadow_root).unwrap().children[0];
    assert!(doc.get_node(shadow_div).unwrap().primary_styles().is_some());
}

#[test]
fn named_and_default_slots_distribute_light_children() {
    let (mut doc, body) = doc_with_body();
    let mut mutator = doc.mutate();

    let host = mutator.create_element(qname("my-host"), Vec::new());
    mutator.set_attribute(host, qname("style"), "display:block");

    // Two light children: one targeting a named slot, one for the default slot.
    let header = mutator.create_element(qname("h1"), Vec::new());
    mutator.set_attribute(header, qname("slot"), "title");
    let para = mutator.create_element(qname("p"), Vec::new());
    mutator.append_children(host, &[header, para]);
    mutator.append_children(body, &[host]);

    // Shadow tree: a named <slot name="title"> and a default <slot>.
    let shadow_root = mutator.attach_shadow(host, ShadowRootMode::Open);
    let named_slot = mutator.create_element(qname("slot"), Vec::new());
    mutator.set_attribute(named_slot, qname("name"), "title");
    let default_slot = mutator.create_element(qname("slot"), Vec::new());
    mutator.append_children(shadow_root, &[named_slot, default_slot]);
    drop(mutator);

    doc.resolve(0.0);

    assert_eq!(
        doc.get_node(header)
            .unwrap()
            .element_data()
            .unwrap()
            .assigned_slot,
        Some(named_slot),
        "slot=\"title\" child assigned to the named slot"
    );
    assert_eq!(
        doc.get_node(para)
            .unwrap()
            .element_data()
            .unwrap()
            .assigned_slot,
        Some(default_slot),
        "unslotted child assigned to the default slot"
    );
}

#[test]
fn unfilled_slot_renders_fallback_content() {
    let (mut doc, body) = doc_with_body();
    let mut mutator = doc.mutate();

    // Host with no light-DOM children, so the slot has nothing to project.
    let host = mutator.create_element(qname("my-host"), Vec::new());
    mutator.set_attribute(host, qname("style"), "display:block");
    mutator.append_children(body, &[host]);

    let shadow_root = mutator.attach_shadow(host, ShadowRootMode::Open);
    let slot = mutator.create_element(qname("slot"), Vec::new());
    let fallback = mutator.create_element(qname("div"), Vec::new());
    mutator.set_attribute(
        fallback,
        qname("style"),
        "display:block;width:33px;height:22px",
    );
    mutator.append_children(slot, &[fallback]);
    mutator.append_children(shadow_root, &[slot]);
    drop(mutator);

    doc.resolve(0.0);

    // With nothing assigned, the slot falls back to laying out its own children.
    let fallback_node = doc.get_node(fallback).unwrap();
    assert!(fallback_node.primary_styles().is_some());
    assert_eq!(fallback_node.final_layout.size.width, 33.0);
    assert_eq!(fallback_node.final_layout.size.height, 22.0);
}

#[test]
fn shadow_style_element_styles_shadow_content() {
    let (mut doc, body) = doc_with_body();
    let mut mutator = doc.mutate();

    let host = mutator.create_element(qname("my-host"), Vec::new());
    mutator.set_attribute(host, qname("style"), "display:block");
    mutator.append_children(body, &[host]);

    let shadow_root = mutator.attach_shadow(host, ShadowRootMode::Open);

    // A <style> element inside the shadow tree.
    let style = mutator.create_element(qname("style"), Vec::new());
    let css = mutator.create_text_node("div { width: 60px; height: 12px; display: block; }");
    mutator.append_children(style, &[css]);

    let div = mutator.create_element(qname("div"), Vec::new());
    mutator.append_children(shadow_root, &[style, div]);
    drop(mutator);

    doc.resolve(0.0);

    let div_node = doc.get_node(div).unwrap();
    assert_eq!(div_node.final_layout.size.width, 60.0);
    assert_eq!(div_node.final_layout.size.height, 12.0);
}

#[test]
fn custom_element_disconnected_on_removal() {
    let (mut doc, body) = doc_with_body();

    let log = Rc::new(RefCell::new(Vec::new()));
    let log_clone = log.clone();
    doc.define_custom_element(
        LocalName::from("greet-box"),
        CustomElementDefinition::new(move || {
            Box::new(GreetWidget {
                log: log_clone.clone(),
            })
        }),
    );

    let mut mutator = doc.mutate();
    let host = mutator.create_element(qname("greet-box"), Vec::new());
    mutator.append_children(body, &[host]);
    drop(mutator);

    assert!(log.borrow().iter().any(|s| s == "connected"));

    let mut mutator = doc.mutate();
    mutator.remove_and_drop_node(host);
    drop(mutator);

    assert!(
        log.borrow().iter().any(|s| s == "disconnected"),
        "disconnected callback ran on removal"
    );
}
