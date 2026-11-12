//! Emulation of the parts of wptserve's static file handler which change what a
//! test sees: `.sub.` template substitution and `.headers` files.
//!
//! See `tools/wptserve/wptserve/{handlers,pipes}.py` in the WPT repository.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use log::warn;
use regex::bytes::{Captures, Regex};
use url::Url;

/// wptserve's default `browser_host`
pub const BROWSER_HOST: &str = "web-platform.test";
/// wptserve's default `alternate_hosts["alt"]`
pub const ALT_HOST: &str = "not-web-platform.test";
/// The origin tests are loaded from (`http://{{host}}:{{ports[http][0]}}`)
pub const ORIGIN: &str = "http://web-platform.test:8000/";

/// wptserve's default port config, with fixed values for the ports it allocates
/// dynamically ("auto")
const PORTS: &[(&str, &[u16])] = &[
    ("http", &[8000, 8001]),
    ("http-local", &[8002]),
    ("http-public", &[8003]),
    ("https", &[8443, 8444]),
    ("https-local", &[8445]),
    ("https-public", &[8446]),
    ("h2", &[9000]),
    ("ws", &[8888]),
    ("wss", &[8889]),
    ("webtransport-h3", &[11000]),
    ("websocket-h3", &[11001]),
    ("dns", &[8053]),
];

/// wptserve's `subdomains` labels, with their IDNA encodings
const SUBDOMAIN_LABELS: &[(&str, &str)] = &[
    ("www", "www"),
    ("www1", "www1"),
    ("www2", "www2"),
    ("天気の良い日", "xn--n8j6ds53lwwkrqhv28a"),
    ("élève", "xn--lve-6lad"),
];
/// wptserve's `not_subdomains` labels
const NOT_SUBDOMAIN_LABELS: &[(&str, &str)] = &[("nonexistent", "nonexistent")];

/// Subdomain name -> IDNA-encoded subdomain, for wptserve's `subdomains` and
/// `not_subdomains` (each label set combined up to two levels deep)
static SUBDOMAINS: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    for labels in [SUBDOMAIN_LABELS, NOT_SUBDOMAIN_LABELS] {
        for (name, encoded) in labels {
            map.insert(name.to_string(), encoded.to_string());
            for (name2, encoded2) in labels {
                map.insert(format!("{name}.{name2}"), format!("{encoded}.{encoded2}"));
            }
        }
    }
    map
});

/// A file as wptserve would serve it
pub struct Response {
    pub body: Vec<u8>,
    pub headers: Vec<(String, String)>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .rev()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    /// The text of the response, decoded using the charset from its
    /// `Content-Type` header (with BOM sniffing) if it has one, else as UTF-8.
    /// Returns `None` for a response without a charset which isn't valid UTF-8.
    pub fn text(self) -> Option<String> {
        let encoding = self
            .header("Content-Type")
            .and_then(content_type_charset)
            .and_then(|label| encoding_rs::Encoding::for_label(label.as_bytes()));
        match encoding {
            Some(encoding) => Some(encoding.decode(&self.body).0.into_owned()),
            None => String::from_utf8(self.body).ok(),
        }
    }
}

/// Serve the file at `relative_path` (relative to the WPT root, no query) for a
/// request to `request_url`, applying the file's `.headers` and `.sub.`
/// substitution (or a `?pipe=sub` query) like wptserve.
pub fn serve(wpt_dir: &Path, relative_path: &str, request_url: &Url) -> std::io::Result<Response> {
    let mut body = std::fs::read(wpt_dir.join(relative_path))?;
    let headers = load_headers(wpt_dir, relative_path, request_url);
    if let Some(escape) = sub_escape(relative_path, request_url) {
        body = template(wpt_dir, relative_path, request_url, &body, escape);
    }
    Ok(Response { body, headers })
}

/// Whether wptserve would serve `relative_path` (a WPT-root-relative path,
/// optionally with a query) with an XML content type. `None` if its `.headers`
/// don't set a `Content-Type`, in which case the file extension decides.
pub fn headers_say_xml(wpt_dir: &Path, relative_path: &str, request_url: &Url) -> Option<bool> {
    let path = relative_path
        .split(['?', '#'])
        .next()
        .unwrap_or(relative_path);
    let headers = load_headers(wpt_dir, path, request_url);
    let content_type = headers
        .iter()
        .rev()
        .find(|(key, _)| key.eq_ignore_ascii_case("Content-Type"))?;
    let mime = content_type
        .1
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    Some(mime.ends_with("+xml") || mime == "application/xml" || mime == "text/xml")
}

/// The `charset` parameter of a `Content-Type` header value
fn content_type_charset(content_type: &str) -> Option<&str> {
    content_type.split(';').skip(1).find_map(|param| {
        let (key, value) = param.split_once('=')?;
        key.trim()
            .eq_ignore_ascii_case("charset")
            .then(|| value.trim().trim_matches('"'))
    })
}

/// Load the headers from the neighbouring `__dir__{.sub}.headers` and
/// `<file>{.sub}.headers` files, in that order.
pub fn load_headers(
    wpt_dir: &Path,
    relative_path: &str,
    request_url: &Url,
) -> Vec<(String, String)> {
    let dir = Path::new(relative_path).parent().unwrap_or(Path::new(""));
    let mut headers = load_headers_file(wpt_dir, &dir.join("__dir__"), request_url);
    headers.extend(load_headers_file(
        wpt_dir,
        Path::new(relative_path),
        request_url,
    ));
    headers
}

fn load_headers_file(wpt_dir: &Path, path: &Path, request_url: &Url) -> Vec<(String, String)> {
    let with_suffix = |suffix: &str| {
        let mut path = path.as_os_str().to_owned();
        path.push(suffix);
        PathBuf::from(path)
    };
    let sub_path = with_suffix(".sub.headers");
    let (headers_path, use_sub) = if wpt_dir.join(&sub_path).exists() {
        (sub_path, true)
    } else {
        (with_suffix(".headers"), false)
    };
    let Ok(mut data) = std::fs::read(wpt_dir.join(&headers_path)) else {
        return Vec::new();
    };
    if use_sub {
        let relative_path = headers_path.to_string_lossy().replace('\\', "/");
        data = template(wpt_dir, &relative_path, request_url, &data, Escape::None);
    }
    String::from_utf8_lossy(&data)
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Escape {
    Html,
    None,
}

/// The escaping to substitute with if the response needs the `sub` pipe
fn sub_escape(relative_path: &str, request_url: &Url) -> Option<Escape> {
    let file_name = relative_path.rsplit('/').next().unwrap_or(relative_path);
    if file_name.contains(".sub.") {
        let extension = file_name.rsplit('.').next().unwrap_or_default();
        let is_markup = ["html", "htm", "xht", "xhtml", "xml", "svg"].contains(&extension);
        return Some(if is_markup {
            Escape::Html
        } else {
            Escape::None
        });
    }
    // Only the `sub` pipe is supported from `?pipe=`
    let pipe = request_url
        .query_pairs()
        .filter(|(key, _)| key == "pipe")
        .last()?
        .1;
    pipe.split('|').find_map(|pipe| match pipe.trim() {
        "sub" | "sub(html)" => Some(Escape::Html),
        "sub(none)" => Some(Escape::None),
        _ => None,
    })
}

static TEMPLATE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{\{([^}]*)\}\}").unwrap());

/// Apply wptserve's `sub` pipe to `content`. Expressions which can't be
/// evaluated are left in place.
fn template(
    wpt_dir: &Path,
    relative_path: &str,
    request_url: &Url,
    content: &[u8],
    escape: Escape,
) -> Vec<u8> {
    let mut ctx = TemplateCtx {
        wpt_dir,
        relative_path,
        request_url,
        variables: HashMap::new(),
    };
    TEMPLATE_RE
        .replace_all(content, |captures: &Captures| {
            let expr = String::from_utf8_lossy(&captures[1]);
            match ctx.eval(&expr) {
                Ok(value) => match escape {
                    Escape::Html => escape_html(&value).into_bytes(),
                    Escape::None => value.into_bytes(),
                },
                Err(err) => {
                    warn!("{relative_path}: can't substitute {{{{{expr}}}}}: {err}");
                    captures[0].to_vec()
                }
            }
        })
        .into_owned()
}

/// Python's `html.escape(value, quote=True)`
fn escape_html(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#x27;"),
            c => out.push(c),
        }
    }
    out
}

#[derive(Debug, PartialEq)]
enum Token {
    Var(String),
    Ident(String),
    Index(String),
    Arguments(Vec<String>),
}

/// wptserve's `ReplacementTokenizer`. Like Python's `re.Scanner`, stops at the
/// first character no token matches.
fn tokenize(expr: &str) -> Vec<Token> {
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut tokens = Vec::new();
    let mut rest = expr;
    loop {
        let word_len = |s: &str| s.find(|c| !is_word(c)).unwrap_or(s.len());
        if let Some(after) = rest.strip_prefix('$') {
            let len = word_len(after);
            if len > 0 && after[len..].starts_with(':') {
                tokens.push(Token::Var(format!("${}", &after[..len])));
                rest = &after[len + 1..];
                continue;
            }
            if len > 0 {
                tokens.push(Token::Ident(format!("${}", &after[..len])));
                rest = &after[len..];
                continue;
            }
            break;
        }
        let len = word_len(rest);
        if len > 0 {
            tokens.push(Token::Ident(rest[..len].to_string()));
            rest = &rest[len..];
        } else if let Some(inner) = rest.strip_prefix('[')
            && let Some(end) = inner.find(']')
        {
            tokens.push(Token::Index(inner[..end].to_string()));
            rest = &inner[end + 1..];
        } else if let Some(inner) = rest.strip_prefix('(')
            && let Some(end) = inner.find(')')
        {
            let args = &inner[..end];
            let args = if args.is_empty() {
                Vec::new()
            } else {
                args.split(',')
                    .map(|arg| arg.trim_start().to_string())
                    .collect()
            };
            tokens.push(Token::Arguments(args));
            rest = &inner[end + 1..];
        } else {
            break;
        }
    }
    tokens
}

enum Value {
    Str(String),
    /// `hosts`: all domains, by host name ("" or "alt") then subdomain
    Hosts,
    /// The domains of one host, by subdomain
    Domains(&'static str),
    Ports,
    PortList(&'static [u16]),
    Location,
    Get,
    Headers,
    Function(String),
}

struct TemplateCtx<'a> {
    wpt_dir: &'a Path,
    relative_path: &'a str,
    request_url: &'a Url,
    variables: HashMap<String, String>,
}

impl TemplateCtx<'_> {
    fn eval(&mut self, expr: &str) -> Result<String, String> {
        let mut tokens = tokenize(expr).into_iter();
        let mut token = tokens.next().ok_or("empty expression")?;
        let variable = match token {
            Token::Var(name) => {
                token = tokens.next().ok_or("missing value")?;
                Some(name)
            }
            _ => None,
        };
        let Token::Ident(field) = token else {
            return Err(format!("expected ident, got {token:?}"));
        };
        let mut value = match field.as_str() {
            _ if self.variables.contains_key(&field) => Value::Str(self.variables[&field].clone()),
            "uuid" | "file_hash" | "fs_path" | "header_or_default" => Value::Function(field),
            "headers" => Value::Headers,
            "GET" => Value::Get,
            "hosts" => Value::Hosts,
            "domains" => Value::Domains(BROWSER_HOST),
            "host" | "browser_host" => Value::Str(BROWSER_HOST.to_string()),
            "ports" => Value::Ports,
            "location" => Value::Location,
            "url_base" => Value::Str("/".to_string()),
            _ => return Err(format!("undefined template variable {field}")),
        };
        for token in tokens {
            value = match (value, token) {
                (Value::Hosts, Token::Index(name)) => match name.as_str() {
                    "" => Value::Domains(BROWSER_HOST),
                    "alt" => Value::Domains(ALT_HOST),
                    _ => return Err(format!("unknown host {name:?}")),
                },
                (Value::Domains(host), Token::Index(subdomain)) => {
                    if subdomain.is_empty() {
                        Value::Str(host.to_string())
                    } else {
                        let encoded = SUBDOMAINS
                            .get(&subdomain)
                            .ok_or_else(|| format!("unknown subdomain {subdomain:?}"))?;
                        Value::Str(format!("{encoded}.{host}"))
                    }
                }
                (Value::Ports, Token::Index(protocol)) => Value::PortList(
                    PORTS
                        .iter()
                        .find(|(name, _)| *name == protocol)
                        .ok_or_else(|| format!("unknown protocol {protocol:?}"))?
                        .1,
                ),
                (Value::PortList(ports), Token::Index(index)) => Value::Str(
                    index
                        .parse::<usize>()
                        .ok()
                        .and_then(|index| ports.get(index))
                        .ok_or_else(|| format!("port index {index:?} out of range"))?
                        .to_string(),
                ),
                (Value::Location, Token::Index(key)) => Value::Str(self.location(&key)?),
                (Value::Get, Token::Index(key)) => Value::Str(
                    self.request_url
                        .query_pairs()
                        .find(|(name, _)| *name == key)
                        .map(|(_, value)| value.into_owned())
                        .unwrap_or_default(),
                ),
                // There are no request headers
                (Value::Headers, Token::Index(name)) => {
                    return Err(format!("no request header {name:?}"));
                }
                (Value::Function(name), Token::Arguments(args)) => {
                    Value::Str(self.call(&name, &args)?)
                }
                (_, token) => return Err(format!("unexpected token {token:?}")),
            };
        }
        let Value::Str(value) = value else {
            return Err("expression doesn't evaluate to a string".to_string());
        };
        if let Some(variable) = variable {
            self.variables.insert(variable, value.clone());
        }
        Ok(value)
    }

    fn location(&self, key: &str) -> Result<String, String> {
        let url = self.request_url;
        let hostname = url.host_str().unwrap_or_default();
        let port = url.port_or_known_default().unwrap_or_default();
        Ok(match key {
            "server" => format!("{}://{hostname}:{port}", url.scheme()),
            "scheme" => url.scheme().to_string(),
            "host" => format!("{hostname}:{port}"),
            "hostname" => hostname.to_string(),
            "port" => port.to_string(),
            "path" | "pathname" => url.path().to_string(),
            "query" => format!("?{}", url.query().unwrap_or_default()),
            _ => return Err(format!("unknown location key {key:?}")),
        })
    }

    fn call(&self, name: &str, args: &[String]) -> Result<String, String> {
        match (name, args) {
            ("uuid", []) => Ok(uuid::Uuid::new_v4().to_string()),
            ("header_or_default", [_, default]) => Ok(default.clone()),
            ("fs_path", [path]) => {
                let root_relative = match path.strip_prefix('/') {
                    Some(path) => path.to_string(),
                    None => match self.relative_path.rsplit_once('/') {
                        Some((dir, _)) => format!("{dir}/{path}"),
                        None => path.clone(),
                    },
                };
                let root_relative = root_relative.replace('/', std::path::MAIN_SEPARATOR_STR);
                Ok(self
                    .wpt_dir
                    .join(root_relative)
                    .to_string_lossy()
                    .into_owned())
            }
            _ => Err(format!("unsupported function {name}({})", args.join(", "))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sub(content: &str, request_url: &str, escape: Escape) -> String {
        let url = Url::parse(request_url).unwrap();
        let out = template(
            Path::new("/wpt"),
            "css/test.sub.html",
            &url,
            content.as_bytes(),
            escape,
        );
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn substitutes_server_config() {
        let url = "http://web-platform.test:8000/css/test.sub.html?allow=a%20b";
        assert_eq!(
            sub(
                "http://{{host}}:{{ports[http][0]}} {{ports[https][1]}} {{hosts[alt][]}} {{hosts[][www]}} {{domains[www2]}} {{hosts[alt][www.www1]}}",
                url,
                Escape::Html
            ),
            "http://web-platform.test:8000 8444 not-web-platform.test www.web-platform.test www2.web-platform.test www.www1.not-web-platform.test"
        );
        assert_eq!(
            sub("{{domains[élève]}}", url, Escape::None),
            "xn--lve-6lad.web-platform.test"
        );
        assert_eq!(
            sub(
                "{{location[server]}}|{{location[host]}}|{{location[path]}}|{{location[query]}}",
                url,
                Escape::None
            ),
            "http://web-platform.test:8000|web-platform.test:8000|/css/test.sub.html|?allow=a%20b"
        );
        assert_eq!(
            sub("{{GET[allow]}}|{{GET[missing]}}", url, Escape::Html),
            "a b|"
        );
        assert_eq!(
            sub("{{header_or_default(X-Test, absent)}}", url, Escape::Html),
            "absent"
        );
        assert_eq!(
            sub("{{fs_path(a.css)}}", url, Escape::Html),
            Path::new("/wpt")
                .join("css")
                .join("a.css")
                .to_string_lossy()
        );
    }

    #[test]
    fn variables_escaping_and_errors() {
        let url = "http://web-platform.test:8000/css/test.sub.html";
        let out = sub("{{$id:uuid()}} {{$id}}", url, Escape::Html);
        let (a, b) = out.split_once(' ').unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 36);
        assert_eq!(&a[14..15], "4");
        assert_eq!(
            sub(
                "{{GET[q]}}",
                "http://web-platform.test:8000/a?q=%3Cb%3E%22",
                Escape::Html
            ),
            "&lt;b&gt;&quot;"
        );
        assert_eq!(
            sub(
                "{{GET[q]}}",
                "http://web-platform.test:8000/a?q=%3Cb%3E",
                Escape::None
            ),
            "<b>"
        );
        assert_eq!(
            sub("{{nope}} {{ports[ftp][0]}}", url, Escape::Html),
            "{{nope}} {{ports[ftp][0]}}"
        );
    }

    #[test]
    fn sub_pipe_selection() {
        let url = Url::parse(ORIGIN).unwrap();
        assert_eq!(sub_escape("a/b.sub.html", &url), Some(Escape::Html));
        assert_eq!(sub_escape("a/b.sub.css", &url), Some(Escape::None));
        assert_eq!(sub_escape("a.sub/b.html", &url), None);
        let piped = url.join("a/b.js?pipe=header(X,y)|sub").unwrap();
        assert_eq!(sub_escape("a/b.js", &piped), Some(Escape::Html));
        let piped = url.join("a/b.js?pipe=trickle(d1)").unwrap();
        assert_eq!(sub_escape("a/b.js", &piped), None);
    }

    #[test]
    fn serves_headers_files() {
        let dir = tempfile::tempdir().unwrap();
        let css = dir.path().join("css");
        std::fs::create_dir(&css).unwrap();
        std::fs::write(css.join("__dir__.headers"), "X-Dir: 1\n").unwrap();
        std::fs::write(css.join("a.xht"), b"caf\xe9").unwrap();
        std::fs::write(
            css.join("a.xht.sub.headers"),
            "Content-Type: application/xhtml+xml; charset=iso-8859-1\nX-Host: {{host}}\n",
        )
        .unwrap();
        std::fs::write(css.join("b.html"), b"caf\xe9").unwrap();

        let url = Url::parse(ORIGIN).unwrap().join("css/a.xht").unwrap();
        let response = serve(dir.path(), "css/a.xht", &url).unwrap();
        assert_eq!(response.header("x-dir"), Some("1"));
        assert_eq!(response.header("X-Host"), Some(BROWSER_HOST));
        assert_eq!(response.text().as_deref(), Some("café"));
        assert_eq!(headers_say_xml(dir.path(), "css/a.xht?x", &url), Some(true));

        let url = Url::parse(ORIGIN).unwrap().join("css/b.html").unwrap();
        assert_eq!(serve(dir.path(), "css/b.html", &url).unwrap().text(), None);
        assert_eq!(headers_say_xml(dir.path(), "css/b.html", &url), None);
    }

    #[test]
    fn tokenizes_like_wptserve() {
        assert_eq!(
            tokenize("$id:file_hash(sha256, a/b.js)"),
            [
                Token::Var("$id".into()),
                Token::Ident("file_hash".into()),
                Token::Arguments(vec!["sha256".into(), "a/b.js".into()]),
            ]
        );
        assert_eq!(
            tokenize("hosts[][www]"),
            [
                Token::Ident("hosts".into()),
                Token::Index("".into()),
                Token::Index("www".into()),
            ]
        );
    }
}
