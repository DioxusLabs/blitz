use comrak::{Options, markdown_to_html_with_plugins, options};

pub(crate) fn markdown_to_html(contents: String) -> String {
    #[allow(unused_mut)]
    let mut plugins = options::Plugins::default();

    #[cfg(feature = "syntax-highlighting-giallo")]
    use giallo_highlighter::{GialloAdapter, ThemeVariant};
    #[cfg(feature = "syntax-highlighting-giallo")]
    let syntax_highligher = GialloAdapter(ThemeVariant::Single("github-light"));
    #[cfg(feature = "syntax-highlighting-giallo")]
    {
        plugins.render.codefence_syntax_highlighter = Some(&syntax_highligher as _);
    }

    let body_html = markdown_to_html_with_plugins(
        &contents,
        &Options {
            extension: options::Extension {
                strikethrough: true,
                tagfilter: false,
                table: true,
                autolink: true,
                tasklist: true,
                superscript: false,
                header_ids: None,
                footnotes: false,
                description_lists: false,
                front_matter_delimiter: None,
                multiline_block_quotes: false,
                alerts: true,
                ..options::Extension::default()
            },
            render: options::Render {
                r#unsafe: true,
                tasklist_classes: true,
                ..options::Render::default()
            },
            ..Options::default()
        },
        &plugins,
    );

    // Strip trailing newlines in code blocks
    let body_html = body_html.replace("\n</code", "</code");

    format!(
        r#"
        <!DOCTYPE html>
        <html>
        <body>
        <div class="markdown-body">{body_html}</div>
        </body>
        </html>
        "#
    )
}

#[cfg(feature = "syntax-highlighting-giallo")]
mod giallo_highlighter {
    use comrak::adapters::SyntaxHighlighterAdapter;
    pub(crate) use giallo::ThemeVariant;
    use giallo::{HighlightOptions, HtmlRenderer, PLAIN_GRAMMAR_NAME, RenderOptions};
    use std::borrow::Cow;
    use std::collections::HashMap;

    static GIALLO_REGISTRY: std::sync::LazyLock<giallo::Registry> =
        std::sync::LazyLock::new(|| {
            let mut registry = giallo::Registry::builtin().unwrap();
            registry.link_grammars();
            registry
        });

    pub(crate) struct GialloAdapter(pub(crate) ThemeVariant<&'static str>);

    impl SyntaxHighlighterAdapter for GialloAdapter {
        fn write_highlighted(
            &self,
            output: &mut dyn std::fmt::Write,
            lang: Option<&str>,
            code: &str,
        ) -> std::fmt::Result {
            let norm_lang = lang.map(|l| l.split_once(',').map(|(lang, _)| lang).unwrap_or(l));
            let norm_lang = norm_lang.unwrap_or(PLAIN_GRAMMAR_NAME);
            let options = HighlightOptions::new(&norm_lang, self.0);
            let highlighted = GIALLO_REGISTRY.highlight(code, options).unwrap();
            let render_options = RenderOptions {
                show_line_numbers: false,
                ..Default::default()
            };
            let html = HtmlRenderer::default().render(&highlighted, &render_options);
            println!("{}", &html);
            output.write_str(&html)
        }

        fn write_pre_tag(
            &self,
            output: &mut dyn std::fmt::Write,
            attributes: HashMap<&'static str, Cow<'_, str>>,
        ) -> std::fmt::Result {
            let _ = attributes;
            output.write_str("")
        }

        fn write_code_tag(
            &self,
            output: &mut dyn std::fmt::Write,
            attributes: HashMap<&'static str, Cow<'_, str>>,
        ) -> std::fmt::Result {
            let _ = attributes;
            output.write_str("")
        }
    }
}

// #[cfg(feature = "syntax-highlighting-syntect")]
// mod syntax_highlighter {
//     use comrak::adapters::SyntaxHighlighterAdapter;
//     use comrak::plugins::syntect::SyntectAdapter;
//     use std::collections::HashMap;

//     struct CustomSyntectAdapter(SyntectAdapter);

//     impl SyntaxHighlighterAdapter for CustomSyntectAdapter {
//         fn write_highlighted(
//             &self,
//             output: &mut dyn std::io::Write,
//             lang: Option<&str>,
//             code: &str,
//         ) -> std::io::Result<()> {
//             let norm_lang = lang.map(|l| l.split_once(',').map(|(lang, _)| lang).unwrap_or(l));
//             self.0.write_highlighted(output, norm_lang, code)
//         }

//         fn write_pre_tag(
//             &self,
//             output: &mut dyn std::io::Write,
//             attributes: HashMap<String, String>,
//         ) -> std::io::Result<()> {
//             self.0.write_pre_tag(output, attributes)
//         }

//         fn write_code_tag(
//             &self,
//             output: &mut dyn std::io::Write,
//             attributes: HashMap<String, String>,
//         ) -> std::io::Result<()> {
//             self.0.write_code_tag(output, attributes)
//         }
//     }
// }
