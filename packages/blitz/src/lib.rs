use std::ops::Deref;
use std::sync::Arc;

use dioxus::dioxus_core::{Component, VirtualDom};
use dioxus_native_core::prelude::*;

use blitz_core::EventData;
use blitz_core::{render, Config, Driver};

pub async fn launch(app: Component<()>) {
    launch_cfg(app, Config).await
}

pub async fn launch_cfg(app: Component<()>, cfg: Config) {
    launch_cfg_with_props(app, (), cfg).await
}

pub async fn launch_cfg_with_props<Props: 'static +Clone+ Send>(
    app: Component<Props>,
    props: Props,
    cfg: Config,
) {
    render(
        move |rdom, _| {
            let mut vdom = VirtualDom::new_with_props(app, props);
            let mut rdom = rdom.write().unwrap();
            let mut dioxus_state = DioxusState::create(&mut rdom);
            vdom.rebuild(&mut dioxus_state.create_mutation_writer(&mut rdom));
            DioxusRenderer {
                vdom,
                dioxus_state,
                #[cfg(all(feature = "hot-reload", debug_assertions))]
                hot_reload_rx: {
                    let (hot_reload_tx, hot_reload_rx) =
                        tokio::sync::mpsc::unbounded_channel::<dioxus_hot_reload::HotReloadMsg>();
                    dioxus_hot_reload::connect(move |msg| {
                        let _ = hot_reload_tx.send(msg);
                    });
                    hot_reload_rx
                },
            }
        },
        cfg,
    )
    .await;
}

struct DioxusRenderer {
    vdom: VirtualDom,
    dioxus_state: DioxusState,
    #[cfg(all(feature = "hot-reload", debug_assertions))]
    hot_reload_rx: tokio::sync::mpsc::UnboundedReceiver<dioxus_hot_reload::HotReloadMsg>,
}

impl Driver for DioxusRenderer {
    fn update(&mut self, mut root: NodeMut<()>) {
        let rdom = root.real_dom_mut();
        self.vdom.render_immediate(&mut self.dioxus_state.create_mutation_writer( rdom));
    }

    fn handle_event(
        &mut self,
        node: NodeMut<()>,
        event: &str,
        value: Arc<EventData>,
        bubbles: bool,
    ) {
        if let Some(id) = node.mounted_id() {
            self.vdom
                .handle_event(event, value.deref().clone().into_any(), id, bubbles);
        }
    }

    fn poll_async(&mut self) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>> {
        #[cfg(all(feature = "hot-reload", debug_assertions))]
        return Box::pin(async {
            let hot_reload_wait = self.hot_reload_rx.recv();
            let mut hot_reload_msg = None;
            let wait_for_work = self.vdom.wait_for_work();
            tokio::select! {
                Some(msg) = hot_reload_wait => {
                    #[cfg(all(feature = "hot-reload", debug_assertions))]
                    {
                        hot_reload_msg = Some(msg);
                    }
                    #[cfg(not(all(feature = "hot-reload", debug_assertions)))]
                    let () = msg;
                }
                _ = wait_for_work => {}
            }
            // if we have a new template, replace the old one
            if let Some(msg) = hot_reload_msg {
                match msg {
                    dioxus_hot_reload::HotReloadMsg::UpdateTemplate(template) => {
                        self.vdom.replace_template(template);
                    }
                    dioxus_hot_reload::HotReloadMsg::Shutdown => {
                        std::process::exit(0);
                    }
                    _ => {}
                }
            }
        });

        #[cfg(not(all(feature = "hot-reload", debug_assertions)))]
        Box::pin(self.vdom.wait_for_work())
    }
}
