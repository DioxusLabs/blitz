mod markdown;
use blitz_dom::net::Resource;
use blitz_html::HtmlDocument;
use blitz_net::Provider;
use blitz_traits::net::SharedCallback;
use markdown::{markdown_to_html, BLITZ_MD_STYLES, GITHUB_MD_STYLES};
use reqwest::header::HeaderName;

use blitz_shell::{
    create_default_event_loop, BlitzApplication, BlitzShellNetCallback, WindowConfig,
};
use std::env::current_dir;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::runtime::Handle;
use url::Url;

const USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64; rv:60.0) Gecko/20100101 Firefox/81.0";
fn main() {
    let raw_url = std::env::args().nth(1).unwrap_or_else(|| {
        let cwd = current_dir().unwrap();
        format!("{}", cwd.display())
    });

    // Turn on the runtime and enter it
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let _guard = rt.enter();

    let event_loop = create_default_event_loop();
    let proxy = event_loop.create_proxy();

    let (base_url, contents, is_md) = rt.block_on(fetch(raw_url));

    // Process markdown if necessary
    let mut html = contents;
    let mut stylesheets = Vec::new();
    if is_md {
        html = markdown_to_html(html);
        stylesheets.push(String::from(GITHUB_MD_STYLES));
        stylesheets.push(String::from(BLITZ_MD_STYLES));
    }

    // println!("{html}");

    let net_callback = Arc::new(BlitzShellNetCallback::new(proxy));
    let net_provider = Arc::new(Provider::new(
        Handle::current(),
        Arc::clone(&net_callback) as SharedCallback<Resource>,
    ));

    let doc = HtmlDocument::from_html(&html, Some(base_url), stylesheets, net_provider, None);
    let window = WindowConfig::new(doc);

    // Create application
    let mut application = BlitzApplication::new(rt, event_loop.create_proxy());
    application.add_window(window);

    // Run event loop
    event_loop.run_app(&mut application).unwrap()
}

async fn fetch(raw_url: String) -> (String, String, bool) {
    if let Ok(url) = Url::parse(&raw_url) {
        match url.scheme() {
            "file" => fetch_file_path(url.path()),
            _ => {
                let client = reqwest::Client::new();
                let response = client
                    .get(url)
                    .header("User-Agent", USER_AGENT)
                    .send()
                    .await
                    .unwrap();
                let content_type = response
                    .headers()
                    .get(HeaderName::from_static("content-type"));
                let is_md = raw_url.ends_with(".md")
                    || content_type.is_some_and(|ct| {
                        ct.to_str().is_ok_and(|ct| ct.starts_with("text/markdown"))
                    });
                let file_content = response.text().await.unwrap();

                (raw_url, file_content, is_md)
            }
        }
    } else if fs::exists(&raw_url).unwrap() {
        fetch_file_path(&raw_url)
    } else {
        eprintln!("Cannot parse {} as url or find it as a file", raw_url);
        std::process::exit(1);
    }
}

fn fetch_file_path(raw_path: &str) -> (String, String, bool) {
    let path = std::path::absolute(Path::new(&raw_path)).unwrap();

    // If path is a directory, search for README.md in that directory or any parent directories
    let path = if path.is_dir() {
        let mut maybe_dir: Option<&Path> = Some(path.as_ref());
        loop {
            match maybe_dir {
                Some(dir) => {
                    let rdme_path = dir.join("README.md");
                    if fs::exists(&rdme_path).unwrap() {
                        break rdme_path;
                    }
                    maybe_dir = dir.parent()
                }
                None => {
                    eprintln!("Could not find README.md file in the current directory");
                    std::process::exit(1);
                }
            }
        }
    } else {
        path
    };

    let base_url_path = path.parent().unwrap().to_string_lossy();
    let base_url = format!("file://{}/", base_url_path);
    let is_md = path.extension() == Some("md".as_ref());

    // Read file
    let file_content = std::fs::read_to_string(&path).unwrap();

    (base_url, file_content, is_md)
}
