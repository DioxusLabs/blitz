# Preact TodoMVC

A minimal [TodoMVC](https://todomvc.com/) app built with [Preact](https://preactjs.com/).

This is a standalone HTML/JavaScript example. It has no build step: open
`index.html` in a browser (or serve the directory with any static file server)
and it runs directly.

## Layout

- `index.html` — the app (markup, styles, and logic using Preact's `h`/hooks).
- `vendor/` — an unmodified copy of the Preact library (UMD builds, v10.29.4),
  loaded via plain `<script>` tags. No bundler or transpiler required.
