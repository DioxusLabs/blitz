# gui-bench

Compares Rust GUI toolkits on identical workloads, timing each phase of a frame
separately. Toolkits: **Dioxus native** (Blitz), **Iced**, **egui**, **Slint**, **GPUI** and **Freya**.

## Workloads

- **table**: the [js-framework-benchmark](https://github.com/krausest/js-framework-benchmark)
  keyed table: create 1,000 rows, replace them, update every 10th row, select a row, swap
  rows 1 and 998, remove a row, create 10,000 rows, append 1,000 to 10,000, clear. Each
  benchmark runs its untimed setup ops, then the timed op, `--warmup` + `--iters` times.
  Rows are `id | label | "x" button`, every row is laid out (no virtualization), and labels
  come from a seeded generator so every toolkit renders the same text.
- **scroll**: a 200-section document (24px bold headings, 16px paragraphs, some italic,
  14px monospace code lines; about 1,000 text blocks), scrolled 20 logical px per frame
  for `--frames` frames after `--warmup` frames.

The window is 1024x768 logical px and not resizable.

## Phases

Every app is driven by `bench-core`'s `Driver`: one op per frame, then the app renders that
frame and writes one JSON line with the time (µs) of each phase it can observe:

| phase | meaning |
|---|---|
| `update` | apply the op to app state, plus the toolkit's reconcile/diff where it has one |
| `build` | style, layout and building the display list / scene / primitives |
| `raster_cpu` | CPU time to encode and submit GPU work |
| `raster_gpu` | GPU time of the frame's render pass, from GPU timestamp queries |
| `acquire` | getting the next swapchain image |
| `present` | presenting it |
| `total` | frame start until present returns |

How each toolkit maps onto these (details in the doc comment at the top of each
`apps/*/src/main.rs`):

| toolkit | update | build | raster_cpu | raster_gpu | acquire / present |
|---|---|---|---|---|---|
| Dioxus native | signal-free model change, root re-render + keyed diff, DOM mutations | Stylo + Taffy + Parley (`resolve`), `paint_scene` into a Vello scene | Vello render, acquire, present **and GPU wait** (anyrender_vello waits every frame) | no (Vello submits its own encoders) | in raster_cpu |
| Iced | model change + `view()` | `UserInterface::build` (diff + layout), `draw` | `Renderer::present` (prepare, encode, submit) | no (iced_wgpu submits its own encoder) | yes |
| egui | model change | the UI closure (immediate mode: widgets, layout and shapes are one pass) + tessellation | egui-wgpu uploads, encode, submit | yes | yes |
| Slint | `VecModel` change notifications | redraw request until `BeforeRendering` | `BeforeRendering` to `AfterRendering` | no (rendering notifier has no command buffers) | present = swap, after `AfterRendering` |
| GPUI | model change + `notify` | `render` + layout + prepaint + paint | scene to Metal/Blade **including present** | no | in raster_cpu |
| Freya | op sent to the app, `State` write, component re-run + diff + tree mutations | Torin layout + render pipeline issuing Skia draw calls | Skia flush, submit **and present** (plugin `BeforePresenting`..`AfterPresenting`) | no (Skia owns its command buffers) | in raster_cpu |

The models differ, so compare `total` across toolkits and use the phases to see where a
toolkit spends its time, not as like-for-like columns. In particular egui and GPUI rebuild
their element trees every frame (there is no keyed diff to measure), Slint evaluates
bindings lazily during rendering, and Dioxus/GPUI fold present into `raster_cpu`.

### Presentation and vsync

egui and Iced present with `AutoNoVsync` by default (`--vsync` switches to `AutoVsync`),
so their `total` is throughput. anyrender_vello (Dioxus) configures `AutoVsync`, Slint uses
its backend's default, and GPUI is driven by the display link, so those are paced to the
display: GPUI's `total` is measured from the start of the frame it draws in, not from the
previous present, so it doesn't include the wait for the next display refresh.

### GPU timing

`raster_gpu` needs `wgpu::Features::TIMESTAMP_QUERY`. Apple's paravirtual GPU in macOS VMs
and most software rasterizers don't have it; the run's `meta` line records
`gpu_timing: timestamp-query | unsupported | disabled | not exposed`. `bench-core`'s
`define_gpu_timer!` works with any wgpu version, so other toolkits can use it if they
expose their render pass or command encoder.

## Running

```sh
# build every app in release mode, run both workloads, write results/<app>-<workload>.jsonl
# and print Markdown tables of median (p95) per phase
cargo run --release -p runner -- run

# a subset, with app arguments after `--`
cargo run --release -p runner -- run --apps egui,iced --workloads scroll -- --frames 2000 --vsync

# summarise existing results
cargo run --release -p runner -- report results/*.jsonl

# one app by hand
cd apps/egui && cargo run --release -- --workload table --iters 10 --warmup 3 --out out.jsonl
```

App arguments: `--workload table|scroll`, `--iters N` (10), `--warmup N` (3),
`--frames N` (600), `--out FILE` (stdout), `--no-gpu-timing`, `--vsync`.

Each app is its own Cargo workspace under `apps/` because the toolkits pin different
wgpu (27 for Iced, 30 for egui and Blitz) and winit (0.30, 0.31 beta for Blitz) versions.
Dioxus native builds the Blitz packages in this repository (path dependencies), so it benchmarks your local checkout.

Close other apps, keep the window visible and unoccluded, and plug in laptops. Report
medians and p95s over several runs, with the machine, OS and the `meta` lines.

### Linux

Install `build-essential pkg-config libssl-dev libfontconfig-dev libfreetype-dev
libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libx11-xcb-dev libxcb1-dev
libvulkan-dev` (see `.github/workflows/gui-bench.yml` at the repository root). On a machine without a GPU, install
Mesa's lavapipe (`mesa-vulkan-drivers`) so the wgpu apps get a software Vulkan adapter,
and run Slint with `SLINT_BACKEND=winit-software`. `update` and `build` are CPU work and
remain comparable there; `raster_*`, `acquire`, `present` and `total` then measure a
software rasterizer, not GPU rendering, so don't compare them with GPU runs.

## Caveats

- Fonts are each toolkit's defaults: egui's bundled Ubuntu/Hack fonts (egui has no bold:
  headings use `strong`), Iced via cosmic-text (its bold sans fallback can pick a monospace
  face), system fonts elsewhere.
- Slint's `Button` and egui's/Iced's buttons are real widgets; Dioxus uses an HTML
  `<button>` and GPUI (which has no button widget) a bordered `div`.
- Built with `lto = "fat"`, `codegen-units = 1`.
