use std::sync::{Arc, Mutex};

use blitz_dom::{Document, DocumentConfig};
use blitz_traits::net::{Bytes, NetHandler, NetProvider, Request};
use blitz_vibey_script::ScriptDocument;

#[derive(Default)]
struct FrameNetProvider {
    requests: Mutex<Vec<String>>,
}

impl NetProvider for FrameNetProvider {
    fn fetch(&self, _: usize, request: Request, handler: Box<dyn NetHandler>) {
        self.requests.lock().unwrap().push(request.url.to_string());
        if request.url.path().ends_with("missing.html") {
            return;
        }
        handler.bytes(
            request.url.to_string(),
            Bytes::from_static(b"<!doctype html><p>Loaded frame</p>"),
        );
    }
}

fn document(script: &str) -> (ScriptDocument, Arc<FrameNetProvider>) {
    let net = Arc::new(FrameNetProvider::default());
    let doc = ScriptDocument::from_html(
        &format!("<!doctype html><body><div id=out></div><script>{script}</script>"),
        DocumentConfig {
            base_url: Some("https://example.test/dir/page.html".to_string()),
            net_provider: Some(net.clone()),
            ..Default::default()
        },
    )
    .without_timer_thread();
    (doc, net)
}

fn output(doc: &ScriptDocument) -> String {
    let inner = doc.inner();
    let id = inner.query_selector("#out").unwrap().unwrap();
    inner.get_node(id).unwrap().text_content()
}

#[test]
fn iframe_attributes_reflect_content_attributes() {
    let (mut doc, net) = document(
        r#"
        const frame = document.createElement('iframe');
        const values = [frame.src, frame.srcdoc, frame.name, frame.width, frame.height, frame.allow];
        frame.src = '../child.html?q=1#part';
        values.push(frame.getAttribute('src'), frame.src, Object.hasOwn(frame, 'src'));
        frame.setAttribute('src', 'other.html');
        values.push(frame.src);
        frame.removeAttribute('src');
        values.push(frame.src);
        frame.src = '';
        values.push(frame.src);
        frame.src = 'http://[';
        values.push(frame.src);
        frame.src = '\ud800';
        values.push(frame.getAttribute('src'), frame.src);
        for (const name of ['srcdoc', 'name', 'width', 'height', 'allow']) {
            frame[name] = name === 'srcdoc' ? '<p>hello</p>' : '50%';
            values.push(frame[name], frame.getAttribute(name));
            frame.setAttribute(name, 'changed');
            values.push(frame[name]);
            frame.removeAttribute(name);
            values.push(frame[name]);
        }
        let threw = false;
        try { frame.src = Symbol(); } catch (e) { threw = e instanceof TypeError; }
        values.push(threw);
        document.getElementById('out').textContent = JSON.stringify(values);
        "#,
    );
    doc.execute_scripts();
    assert_eq!(doc.take_js_errors(), Vec::<String>::new());
    assert_eq!(
        output(&doc),
        concat!(
            r#"["","","","","","","../child.html?q=1#part","https://example.test/child.html?q=1#part",true,"https://example.test/dir/other.html","","https://example.test/dir/page.html","http://[","�","https://example.test/dir/%EF%BF%BD","<p>hello</p>","<p>hello</p>","changed","","50%","50%","changed","","50%","50%","changed","","50%","50%","changed","","50%","50%","changed","",true]"#,
        )
    );
    assert!(net.requests.lock().unwrap().is_empty());
}

#[test]
fn setting_src_loads_frame_and_dispatches_load_once_without_bubbling() {
    let (mut doc, net) = document(
        r#"
        const out = document.getElementById('out');
        const frame = document.createElement('iframe');
        document.body.appendChild(frame);
        document.body.addEventListener('load', () => out.textContent += 'bubble');
        frame.onload = event => {
            out.textContent += `handler:${event.target === frame}:${event.bubbles}:${event.cancelable};`;
        };
        frame.addEventListener('load', function(event) {
            out.textContent += `listener:${this === frame}:${event.currentTarget === frame};`;
        }, {once: true});
        frame.src = 'child.html';
        out.textContent += 'script;';
        "#,
    );
    doc.execute_scripts();
    doc.poll(None);
    doc.poll(None);
    assert_eq!(doc.take_js_errors(), Vec::<String>::new());
    assert_eq!(
        output(&doc),
        "script;listener:true:true;handler:true:false:false;"
    );
    assert_eq!(
        *net.requests.lock().unwrap(),
        ["https://example.test/dir/child.html"]
    );
    let inner = doc.inner();
    let frame = inner.query_selector("iframe").unwrap().unwrap();
    let sub_doc = inner.get_node(frame).unwrap().subdoc().unwrap().inner();
    let p = sub_doc.query_selector("p").unwrap().unwrap();
    assert_eq!(sub_doc.get_node(p).unwrap().text_content(), "Loaded frame");
}

#[test]
fn srcdoc_loads_and_replaced_or_removed_frames_do_not_fire_stale_events() {
    let (mut doc, net) = document(
        r#"
        const frame = document.createElement('iframe');
        frame.onload = () => document.getElementById('out').textContent += 'load;';
        frame.srcdoc = '<p>first</p>';
        document.body.appendChild(frame);
        frame.srcdoc = '<p>last</p>';
        frame.src = 'ignored.html';
        "#,
    );
    doc.execute_scripts();
    doc.poll(None);
    assert_eq!(output(&doc), "load;");
    assert!(net.requests.lock().unwrap().is_empty());
    {
        let inner = doc.inner();
        let id = inner.query_selector("iframe").unwrap().unwrap();
        let sub_doc = inner.get_node(id).unwrap().subdoc().unwrap().inner();
        let p = sub_doc.query_selector("p").unwrap().unwrap();
        assert_eq!(sub_doc.get_node(p).unwrap().text_content(), "last");
    }
    doc.eval("frame.srcdoc = '<p>removed</p>'; frame.remove();");
    doc.poll(None);
    assert_eq!(output(&doc), "load;");
    assert_eq!(doc.take_js_errors(), Vec::<String>::new());
}

#[test]
fn failed_and_superseded_src_loads_do_not_dispatch_load() {
    let (mut doc, _) = document(
        r#"
        const frame = document.createElement('iframe');
        document.body.appendChild(frame);
        frame.onload = () => document.getElementById('out').textContent += 'load;';
        frame.src = 'old.html';
        frame.src = 'missing.html';
        "#,
    );
    doc.execute_scripts();
    doc.poll(None);
    assert_eq!(output(&doc), "");
    doc.eval("frame.src = 'new.html';");
    doc.poll(None);
    doc.poll(None);
    assert_eq!(output(&doc), "load;");
    assert_eq!(doc.take_js_errors(), Vec::<String>::new());
}
