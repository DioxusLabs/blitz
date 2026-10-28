use blitz_dom::{Document, DocumentConfig};
use blitz_vibey_script::ScriptDocument;

fn check(script: &str) {
    let html = format!(
        "<html><body><div id='result'></div><script>{script}\ndocument.getElementById('result').textContent = 'ok';</script></body></html>"
    );
    let mut doc = ScriptDocument::from_html(&html, DocumentConfig::default());
    doc.execute_scripts();
    let inner = doc.inner();
    let id = inner.query_selector("#result").unwrap().unwrap();
    assert_eq!(inner.get_node(id).unwrap().text_content(), "ok");
}

#[test]
fn construction_and_form_encoding() {
    check(
        r#"
        function eq(a, b) { if (JSON.stringify(a) !== JSON.stringify(b)) throw new Error(JSON.stringify([a,b])); }
        eq(new URLSearchParams().size, 0);
        eq(new URLSearchParams(null).toString(), '');
        eq(new URLSearchParams('?a+b=%2B+%FF&empty&bad=%zz&&a+b=2').getAll('a b'), ['+ \uFFFD', '2']);
        eq(new URLSearchParams('x=%EF%BB%BF').get('x'), '\uFEFF');
        eq(new URLSearchParams({a: ' !~*\uD800', b: null}).toString(), 'a=+%21%7E*%EF%BF%BD&b=null');
        const original = new URLSearchParams([['a', 1], ['a', 2]]);
        const copy = new URLSearchParams(original);
        original.append('a', 3);
        eq(copy.getAll('a'), ['1', '2']);
        eq(new URLSearchParams({a: 1}).toString(), 'a=1');
        eq([...new URLSearchParams({'\uD800x': 1, '\uD801x': 2})], [['\uFFFDx', '2']]);
        eq(new URLSearchParams('x=a=b').get('x'), 'a=b');
        eq(new URLSearchParams('x=%zz').get('x'), '%zz');
        "#,
    );
}

#[test]
fn mutations_and_live_iteration() {
    check(
        r#"
        function eq(a, b) { if (JSON.stringify(a) !== JSON.stringify(b)) throw new Error(JSON.stringify([a,b])); }
        const p = new URLSearchParams('b=1&a=2&a=3');
        eq(p.has('a', '3'), true); eq(p.has('a', '4'), false);
        p.delete('a', '2'); eq(p.getAll('a'), ['3']);
        p.append('a', '4'); p.set('a', '5'); eq(p.toString(), 'b=1&a=5');
        p.append('b', '2'); p.sort(); eq([...p], [['a','5'], ['b','1'], ['b','2']]);
        eq([...p.keys()], ['a','b','b']); eq([...p.values()], ['5','1','2']);
        const seen = []; const receiver = {};
        p.forEach(function(value, key, obj) {
            eq(this === receiver && obj === p, true); seen.push(key + value);
            if (seen.length === 1) p.append('c', '6');
        }, receiver);
        eq(seen, ['a5','b1','b2','c6']);
        const iterator = p.entries(); iterator.next(); p.delete('b'); eq(iterator.next().value, ['c','6']);
        p.delete('a'); p.delete('c'); eq(p.size, 0); eq(p.get('missing'), null);
        eq(p.getAll('missing'), []); eq([...p], []);
        "#,
    );
}

#[test]
fn invalid_inputs_and_receivers() {
    check(
        r#"
        function throws(fn) { try { fn(); } catch (e) { if (e instanceof TypeError) return; throw e; } throw new Error('Expected TypeError'); }
        throws(() => URLSearchParams());
        throws(() => new URLSearchParams([['one']]));
        throws(() => new URLSearchParams([['one','two','three']]));
        throws(() => new URLSearchParams(['ab']));
        throws(() => new URLSearchParams(Symbol()));
        throws(() => new URLSearchParams({[Symbol()]: 'x'}));
        throws(() => URLSearchParams.prototype.get.call({}, 'a'));
        const p = new URLSearchParams();
        for (const key of ['get','getAll','has','delete','append','set','forEach']) throws(() => p[key]());
        throws(() => p.forEach(null)); throws(() => p.append('a', Symbol()));
        "#,
    );
}

#[test]
fn url_search_params_are_bidirectionally_live() {
    check(
        r#"
        function eq(a, b) { if (a !== b) throw new Error(a + ' !== ' + b); }
        const url = new URL('https://example.org/?a=b%20c#hash');
        const p = url.searchParams;
        eq(p, url.searchParams); eq(p.get('a'), 'b c');
        p.append('x', '~'); eq(url.search, '?a=b+c&x=%7E');
        url.search = '?z=2'; eq(p.toString(), 'z=2');
        url.href = 'https://example.com/?q=3'; eq(p.get('q'), '3');
        p.delete('q'); eq(url.href, 'https://example.com/');
        url.search = '?'; p.sort(); eq(url.href, 'https://example.com/');
        "#,
    );
}
