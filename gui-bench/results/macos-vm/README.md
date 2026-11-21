# macOS VM run (example output, not representative)

Machine: macOS 26.5.2 VM, Apple M4 (Virtual), 8 cores, 16 GiB, 1024x768 window at scale 1.
GPU: Apple Paravirtual device (Metal) — no timestamp queries, and a virtual GPU, so the
raster/present columns say little about real hardware. Rerun on a physical Mac for real
numbers.

Versions: egui 0.37.0 (wgpu 30), Iced 0.14.0 (wgpu 27), Slint 1.18.1 (default backend:
winit + femtovg on OpenGL), GPUI 0.2.2 (Metal), Freya 0.5.0 (Skia on Metal), Dioxus native from this repository
(Vello 0.11, wgpu 30).

Command: `cargo run --release -p runner -- run --out-dir results/macos-vm` (10 iterations
after 3 warmups per table benchmark; 600 scroll frames after 3 warmups).

Dioxus native, Slint and Freya present with vsync (their `raster_cpu` includes waiting for the
display), egui and Iced without; see the top-level README.

## Runs

| workload | toolkit | renderer | adapter | os |
|---|---|---|---|---|
| scroll | dioxus-native | anyrender_vello (vello) | - | macos |
| scroll | egui | egui-wgpu | Apple Paravirtual device (Metal) | macos |
| scroll | freya | freya skia (Metal) | - | macos |
| scroll | gpui | gpui (Metal) | - | macos |
| scroll | iced | iced_wgpu | Apple Paravirtual device (Metal) | macos |
| scroll | slint | slint default backend (OpenGL) | - | macos |
| table | dioxus-native | anyrender_vello (vello) | - | macos |
| table | egui | egui-wgpu | Apple Paravirtual device (Metal) | macos |
| table | freya | freya skia (Metal) | - | macos |
| table | gpui | gpui (Metal) | - | macos |
| table | iced | iced_wgpu | Apple Paravirtual device (Metal) | macos |
| table | slint | slint default backend (OpenGL) | - | macos |

## scroll: scroll

Median (p95) ms per phase; `-` means the toolkit doesn't expose that phase.

| toolkit | n | update | build | raster_cpu | raster_gpu | acquire | present | total | total p99 | frames > 16.7ms |
|---|---|---|---|---|---|---|---|---|---|---|
| dioxus-native | 600 | 0.00 (0.00) | 0.28 (0.52) | 16.24 (19.58) | - | - | - | 16.54 (19.86) | 20.13 | 276 |
| egui | 600 | 0.00 (0.00) | 0.55 (1.02) | 0.27 (0.58) | - | 0.22 (4.03) | 0.02 (0.10) | 1.86 (5.35) | 10.31 | 2 |
| freya | 600 | 1.02 (1.51) | 0.94 (1.34) | 0.15 (0.25) | - | - | - | 16.57 (19.73) | 31.14 | 276 |
| gpui | 600 | 0.00 (0.00) | 3.79 (4.54) | 0.41 (0.58) | - | - | - | 4.20 (5.06) | 5.71 | 0 |
| iced | 600 | 0.02 (0.06) | 0.08 (0.19) | 0.23 (0.56) | - | 0.12 (4.49) | 0.01 (0.04) | 1.22 (5.11) | 7.36 | 3 |
| slint | 600 | 0.00 (0.00) | 4.23 (6.40) | 11.65 (14.31) | - | - | 0.73 (1.15) | 16.77 (20.34) | 30.58 | 333 |

## table: append_1k_to_10k

Median (p95) ms per phase; `-` means the toolkit doesn't expose that phase.

| toolkit | n | update | build | raster_cpu | raster_gpu | acquire | present | total |
|---|---|---|---|---|---|---|---|---|
| dioxus-native | 10 | 6.99 (7.59) | 21.22 (22.81) | 3.12 (4.16) | - | - | - | 31.83 (33.02) |
| egui | 10 | 0.10 (0.12) | 33.77 (35.28) | 0.29 (0.43) | - | 0.02 (0.05) | 0.02 (0.06) | 34.18 (35.71) |
| freya | 10 | 90.67 (102.42) | 37.45 (44.60) | 0.25 (0.27) | - | - | - | 129.71 (147.32) |
| gpui | 10 | 0.10 (0.11) | 118.06 (122.08) | 11.44 (13.33) | - | - | - | 129.33 (134.22) |
| iced | 10 | 1.12 (1.46) | 156.85 (160.44) | 0.19 (0.40) | - | 0.02 (0.02) | 0.01 (0.08) | 158.19 (161.76) |
| slint | 10 | 0.13 (0.14) | 69.80 (90.38) | 74.04 (77.90) | - | - | 0.75 (1.00) | 146.11 (165.42) |

## table: clear_1k

Median (p95) ms per phase; `-` means the toolkit doesn't expose that phase.

| toolkit | n | update | build | raster_cpu | raster_gpu | acquire | present | total |
|---|---|---|---|---|---|---|---|---|
| dioxus-native | 10 | 2.97 (3.13) | 0.03 (0.06) | 14.93 (17.10) | - | - | - | 17.96 (20.05) |
| egui | 10 | 0.02 (0.02) | 0.05 (0.07) | 0.10 (0.17) | - | 0.02 (0.04) | 0.01 (0.05) | 0.20 (0.30) |
| freya | 10 | 10.38 (14.57) | 0.15 (0.18) | 0.09 (0.19) | - | - | - | 10.68 (14.90) |
| gpui | 10 | 0.02 (0.02) | 0.03 (0.03) | 2.22 (4.55) | - | - | - | 2.27 (4.59) |
| iced | 10 | 0.02 (0.03) | 0.86 (2.03) | 0.07 (0.11) | - | 0.01 (0.06) | 0.01 (0.03) | 0.97 (2.23) |
| slint | 10 | 2.48 (2.71) | 2.25 (7.11) | 0.01 (0.01) | - | - | 1.38 (9.32) | 9.86 (12.38) |

## table: create_10k

Median (p95) ms per phase; `-` means the toolkit doesn't expose that phase.

| toolkit | n | update | build | raster_cpu | raster_gpu | acquire | present | total |
|---|---|---|---|---|---|---|---|---|
| dioxus-native | 10 | 15.56 (25.87) | 107.56 (161.20) | 3.13 (3.78) | - | - | - | 128.34 (179.24) |
| egui | 10 | 0.97 (1.14) | 58.37 (58.77) | 0.32 (0.37) | - | 0.01 (0.14) | 0.02 (0.03) | 59.76 (60.10) |
| freya | 10 | 3164.46 (3267.61) | 125.97 (132.18) | 0.22 (0.60) | - | - | - | 3293.78 (3400.09) |
| gpui | 10 | 0.79 (0.96) | 164.09 (180.76) | 10.14 (12.03) | - | - | - | 175.37 (192.72) |
| iced | 10 | 1.84 (2.24) | 150.74 (152.57) | 0.18 (0.24) | - | 0.02 (0.02) | 0.01 (0.01) | 152.79 (154.66) |
| slint | 10 | 0.90 (1.08) | 135.19 (147.00) | 59.35 (62.45) | - | - | 0.81 (0.91) | 195.31 (211.23) |

## table: create_1k

Median (p95) ms per phase; `-` means the toolkit doesn't expose that phase.

| toolkit | n | update | build | raster_cpu | raster_gpu | acquire | present | total |
|---|---|---|---|---|---|---|---|---|
| dioxus-native | 10 | 1.78 (2.57) | 11.05 (20.50) | 4.60 (6.82) | - | - | - | 18.05 (29.08) |
| egui | 10 | 0.11 (0.12) | 11.00 (15.18) | 0.30 (0.46) | - | 0.25 (1.85) | 0.03 (0.13) | 11.83 (15.66) |
| freya | 10 | 37.25 (39.61) | 19.05 (20.94) | 0.17 (0.21) | - | - | - | 56.76 (60.07) |
| gpui | 10 | 0.10 (0.13) | 23.84 (45.71) | 1.33 (15.35) | - | - | - | 24.88 (61.19) |
| iced | 10 | 0.26 (0.48) | 18.25 (22.75) | 0.21 (2.24) | - | 0.02 (7.65) | 0.01 (0.08) | 18.96 (31.01) |
| slint | 10 | 0.15 (0.28) | 27.14 (29.90) | 11.04 (13.54) | - | - | 0.77 (0.85) | 39.00 (43.78) |

## table: remove_row

Median (p95) ms per phase; `-` means the toolkit doesn't expose that phase.

| toolkit | n | update | build | raster_cpu | raster_gpu | acquire | present | total |
|---|---|---|---|---|---|---|---|---|
| dioxus-native | 10 | 0.47 (0.56) | 0.86 (1.24) | 13.67 (18.78) | - | - | - | 15.02 (20.04) |
| egui | 10 | 0.00 (0.00) | 3.31 (4.83) | 0.25 (0.35) | - | 0.02 (0.11) | 0.02 (0.17) | 3.61 (5.22) |
| freya | 10 | 1.77 (2.95) | 3.12 (4.81) | 0.15 (0.42) | - | - | - | 5.09 (8.25) |
| gpui | 10 | 0.00 (0.01) | 12.37 (14.64) | 1.35 (2.39) | - | - | - | 14.42 (15.99) |
| iced | 10 | 0.10 (0.10) | 0.28 (0.52) | 0.09 (0.18) | - | 0.01 (0.06) | 0.01 (0.02) | 0.51 (0.77) |
| slint | 10 | 0.02 (0.02) | 4.57 (14.41) | 12.25 (15.85) | - | - | 0.51 (17.94) | 20.33 (32.21) |

## table: replace_1k

Median (p95) ms per phase; `-` means the toolkit doesn't expose that phase.

| toolkit | n | update | build | raster_cpu | raster_gpu | acquire | present | total |
|---|---|---|---|---|---|---|---|---|
| dioxus-native | 10 | 4.67 (5.96) | 10.67 (12.79) | 2.65 (6.69) | - | - | - | 17.14 (23.75) |
| egui | 10 | 0.11 (0.11) | 7.41 (8.38) | 0.25 (0.55) | - | 0.03 (3.21) | 0.03 (0.03) | 7.84 (11.94) |
| freya | 10 | 42.81 (47.31) | 19.34 (23.96) | 0.17 (0.36) | - | - | - | 62.54 (69.17) |
| gpui | 10 | 0.11 (0.13) | 17.58 (21.53) | 1.32 (1.94) | - | - | - | 18.97 (22.94) |
| iced | 10 | 0.20 (0.49) | 14.28 (17.26) | 0.14 (0.18) | - | 0.02 (0.03) | 0.01 (0.02) | 14.66 (17.61) |
| slint | 10 | 2.69 (3.20) | 14.39 (17.92) | 11.66 (12.70) | - | - | 0.57 (0.87) | 30.43 (33.03) |

## table: select_row

Median (p95) ms per phase; `-` means the toolkit doesn't expose that phase.

| toolkit | n | update | build | raster_cpu | raster_gpu | acquire | present | total |
|---|---|---|---|---|---|---|---|---|
| dioxus-native | 10 | 0.48 (0.59) | 0.66 (0.82) | 14.14 (17.75) | - | - | - | 15.29 (18.88) |
| egui | 10 | 0.00 (0.00) | 3.52 (4.60) | 0.24 (0.39) | - | 0.05 (0.08) | 0.02 (0.07) | 3.85 (5.04) |
| freya | 10 | 1.54 (2.19) | 2.33 (2.67) | 0.16 (0.26) | - | - | - | 4.19 (4.95) |
| gpui | 10 | 0.00 (0.00) | 10.82 (14.11) | 1.08 (1.72) | - | - | - | 11.84 (15.16) |
| iced | 10 | 0.10 (0.15) | 0.27 (0.35) | 0.08 (0.23) | - | 0.02 (0.02) | 0.00 (0.04) | 0.48 (0.76) |
| slint | 10 | 0.33 (0.46) | 6.74 (8.19) | 12.71 (15.51) | - | - | 0.85 (15.56) | 21.01 (30.30) |

## table: swap_rows

Median (p95) ms per phase; `-` means the toolkit doesn't expose that phase.

| toolkit | n | update | build | raster_cpu | raster_gpu | acquire | present | total |
|---|---|---|---|---|---|---|---|---|
| dioxus-native | 10 | 0.53 (0.59) | 0.90 (1.10) | 14.25 (18.54) | - | - | - | 15.70 (19.83) |
| egui | 10 | 0.00 (0.00) | 3.54 (4.85) | 0.26 (0.61) | - | 0.02 (0.04) | 0.02 (0.09) | 3.87 (5.53) |
| freya | 10 | 2.08 (3.03) | 2.57 (3.57) | 0.16 (0.23) | - | - | - | 5.07 (6.82) |
| gpui | 10 | 0.00 (0.00) | 10.29 (11.99) | 1.06 (1.14) | - | - | - | 11.33 (13.13) |
| iced | 10 | 0.10 (0.10) | 0.32 (0.35) | 0.08 (0.12) | - | 0.02 (0.02) | 0.00 (0.01) | 0.53 (0.57) |
| slint | 10 | 0.31 (0.38) | 3.88 (8.48) | 12.41 (12.87) | - | - | 0.68 (15.05) | 18.99 (29.45) |

## table: update_every_10th

Median (p95) ms per phase; `-` means the toolkit doesn't expose that phase.

| toolkit | n | update | build | raster_cpu | raster_gpu | acquire | present | total |
|---|---|---|---|---|---|---|---|---|
| dioxus-native | 10 | 0.48 (0.59) | 1.67 (1.89) | 13.55 (17.75) | - | - | - | 15.69 (19.85) |
| egui | 10 | 0.00 (0.01) | 4.19 (6.32) | 0.32 (0.60) | - | 0.02 (0.08) | 0.04 (0.19) | 4.67 (6.87) |
| freya | 10 | 1.46 (1.89) | 4.21 (6.65) | 0.14 (0.16) | - | - | - | 5.94 (8.75) |
| gpui | 10 | 0.01 (0.01) | 10.83 (14.78) | 1.08 (1.67) | - | - | - | 11.88 (16.46) |
| iced | 10 | 0.10 (0.13) | 1.84 (1.98) | 0.19 (0.23) | - | 0.01 (0.03) | 0.01 (0.04) | 2.16 (2.31) |
| slint | 10 | 0.37 (0.56) | 5.32 (7.86) | 12.63 (14.88) | - | - | 0.76 (1.88) | 19.29 (23.87) |
