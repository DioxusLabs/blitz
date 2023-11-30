# stylo-dioxus

wgpu + vello + stylo + taffy + tailwindcss + dioxus

## status

work in progress, doesn't compile yet

## goals

interactive HTML/CSS renderer powered by firefox's stylo engine

## the pieces

- wgpu - rendering - relatively stable, broad support, fast, API still in flux
- vello - drawing - good API support, not complete, missing a few performance-related features for animations
- stylo - style - used within production firefox, tied to the firefox project, slightly unwieldy as a dependecy
- taffy - layout - fast, relatively stable, competes well with facebook's Yoga project
- tailwind - css - prebaked stylesheets feed into stylo, no issues, desire to use railwind though
- dioxus - state - relatively stable, single-threaded, lots of rendering infra to borrow from blitz/native-core
