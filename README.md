# stylo-dioxus

wgpu + vello + stylo + taffy + tailwindcss + dioxus

## goals

interactive HTML/CSS renderer powered by firefox's stylo engine

## status
TODO:
- [x] Compute styles for html5ever document
- [ ] Compute layout with Taffy
- [ ] Compute styles for Dioxus Lazy Nodes
- [ ] Pass layout and styles to WGPU for single frame to png
- [ ] Render to window
- [ ] Add interactivity (hit testing, etc etc)
- [ ] Implement per-frame caching
- [ ] Add accessibility kit
- [ ] Multiwindow
- [ ] use_wgpu_context() to grab an element as an arbitrary render surface


## the pieces

- wgpu - rendering - relatively stable, broad support, fast, API still in flux
- vello - drawing - good API support, not complete, missing a few performance-related features for animations
- stylo - style - used within production firefox, tied to the firefox project, slightly unwieldy as a dependecy
- taffy - layout - fast, relatively stable, competes well with facebook's Yoga project
- tailwind - css - prebaked stylesheets feed into stylo, no issues, desire to use railwind though
- dioxus - state - relatively stable, single-threaded, lots of rendering infra to borrow from blitz/native-core


## License

This project is licensed under the MIT license.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in stylo-dioxus by you, shall be licensed as MIT, without any additional terms or conditions.
