# Blitz-dom

This crate implements just the combination of stylo + taffy to provide resolution and layout for any HTML and CSS.

CSS resolution works well (thanks to stylo) but the integration between the two is not yet done.

Notably, not all of servo's concepts map onto taffy's, and we don't have psuedoelements or shadow doms implemented.

In the future, we want this crate to support parallelization since stylo already does. Taffy is currently the limiting factor, as well as generally just designing good multithreaded solutions.
