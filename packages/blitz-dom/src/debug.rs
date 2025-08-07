use parley::layout::PositionedLayoutItem;

use crate::BaseDocument;

impl BaseDocument {
    pub fn print_taffy_tree(&self) {
        taffy::print_tree(self, taffy::NodeId::from(0usize));
    }

    pub fn debug_log_node(&self, node_id: usize) {
        let node = &self.nodes[node_id];

        #[cfg(feature = "tracing")]
        {
            tracing::info!("Layout: {:?}", &node.final_layout);
            tracing::info!("Style: {:?}", &node.style);
        }

        println!("\nNode {} {}", node.id, node.node_debug_str());

        println!("Attrs:");

        for attr in node.attrs().into_iter().flatten() {
            println!("    {}: {}", attr.name.local, attr.value);
        }

        if node.flags.is_inline_root() {
            let inline_layout = &node
                .data
                .downcast_element()
                .unwrap()
                .inline_layout_data
                .as_ref()
                .unwrap();

            println!(
                "Size: {}x{}",
                inline_layout.layout.width(),
                inline_layout.layout.height()
            );
            println!("Text content: {:?}", inline_layout.text);
            println!("Inline Boxes:");
            for ibox in inline_layout.layout.inline_boxes() {
                print!("(id: {}) ", ibox.id);
            }
            println!();
            println!("Lines:");
            for (i, line) in inline_layout.layout.lines().enumerate() {
                println!("Line {i}:");
                for item in line.items() {
                    print!("  ");
                    match item {
                        PositionedLayoutItem::GlyphRun(run) => {
                            print!(
                                "RUN (x: {}, w: {}) ",
                                run.offset().round(),
                                run.run().advance()
                            )
                        }
                        PositionedLayoutItem::InlineBox(ibox) => print!(
                            "BOX (id: {} x: {} y: {} w: {}, h: {})",
                            ibox.id,
                            ibox.x.round(),
                            ibox.y.round(),
                            ibox.width.round(),
                            ibox.height.round()
                        ),
                    }
                    println!();
                }
            }
        }

        let layout = &node.final_layout;
        println!("Layout:");
        println!(
            "  x: {x} y: {y} w: {width} h: {height} content_w: {content_width} content_h: {content_height}",
            x = layout.location.x,
            y = layout.location.y,
            width = layout.size.width,
            height = layout.size.height,
            content_width = layout.content_size.width,
            content_height = layout.content_size.height,
        );
        println!(
            "  border: l:{l} r:{r} t:{t} b:{b}",
            l = layout.border.left,
            r = layout.border.right,
            t = layout.border.top,
            b = layout.border.bottom,
        );
        println!(
            "  padding: l:{l} r:{r} t:{t} b:{b}",
            l = layout.padding.left,
            r = layout.padding.right,
            t = layout.padding.top,
            b = layout.padding.bottom,
        );
        println!(
            "  margin: l:{l} r:{r} t:{t} b:{b}",
            l = layout.margin.left,
            r = layout.margin.right,
            t = layout.margin.top,
            b = layout.margin.bottom,
        );
        println!("Parent: {:?}", node.parent);

        let children: Vec<_> = node
            .children
            .iter()
            .map(|id| &self.nodes[*id])
            .map(|node| (node.id, node.order(), node.node_debug_str()))
            .collect();
        println!("Children: {children:?}");

        println!("Layout Parent: {:?}", node.layout_parent.get());

        let layout_children: Option<Vec<_>> = node.layout_children.borrow().as_ref().map(|lc| {
            lc.iter()
                .map(|id| &self.nodes[*id])
                .map(|node| (node.id, node.order(), node.node_debug_str()))
                .collect()
        });
        if let Some(layout_children) = layout_children {
            println!("Layout Children: {layout_children:?}");
        }

        let paint_children: Option<Vec<_>> = node.paint_children.borrow().as_ref().map(|lc| {
            lc.iter()
                .map(|id| &self.nodes[*id])
                .map(|node| (node.id, node.order(), node.node_debug_str()))
                .collect()
        });
        if let Some(paint_children) = paint_children {
            println!("Paint Children: {paint_children:?}");
        }
        // taffy::print_tree(&self.dom, node_id.into());
    }
}

#[allow(unused_imports)]
pub(crate) use debug_timer::*;

#[cfg(feature = "log_phase_times")]
#[allow(dead_code)]
mod debug_timer {
    use std::io::{Write, stdout};
    use std::time::Instant;

    pub(crate) struct DebugTimer {
        initial_time: Instant,
        last_time: Instant,
        recorded_times: Vec<(&'static str, u64)>,
    }

    impl DebugTimer {
        pub(crate) fn init() -> Self {
            let time = Instant::now();
            Self {
                initial_time: time,
                last_time: time,
                recorded_times: Vec::new(),
            }
        }

        pub(crate) fn record_time(&mut self, message: &'static str) {
            let now = Instant::now();
            let diff = (now - self.last_time).as_millis() as u64;
            self.recorded_times.push((message, diff));
            self.last_time = now;
        }

        pub(crate) fn print_times(&self, message: &str) {
            let now = Instant::now();
            let overall_ms = (now - self.initial_time).as_millis();

            let mut out = stdout().lock();
            write!(out, "{message}{overall_ms}ms (").unwrap();
            for (idx, time) in self.recorded_times.iter().enumerate() {
                if idx != 0 {
                    write!(out, ", ").unwrap();
                }
                write!(out, "{}: {}ms", time.0, time.1).unwrap();
            }
            writeln!(out, ")").unwrap();
        }
    }
}

#[allow(dead_code)]
#[cfg(not(feature = "log_phase_times"))]
mod debug_timer {
    pub(crate) struct DebugTimer;
    impl DebugTimer {
        pub(crate) fn init() -> Self {
            Self
        }
        pub(crate) fn record_time(&mut self, _message: &'static str) {}
        pub(crate) fn print_times(&self, _message: &str) {}
    }
}
