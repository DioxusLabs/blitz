use std::ops::Deref;
use std::sync::Arc;

use dioxus::core::{Component, VirtualDom};
use dioxus_native_core::{
    dioxus::{DioxusState, NodeImmutableDioxusExt},
    Renderer,
};

use crate::events::EventData;
use crate::{render, Config};

pub async fn launch(app: Component<()>) {
    launch_cfg(app, Config::default()).await
}

pub async fn launch_cfg(app: Component<()>, cfg: Config) {
    launch_cfg_with_props(app, (), cfg).await
}

pub async fn launch_cfg_with_props<Props: 'static + Send>(
    app: Component<Props>,
    props: Props,
    cfg: Config,
) {
    render(
        move |rdom, _| {
            let mut vdom = VirtualDom::new_with_props(app, props);
            let muts = vdom.rebuild();
            let mut rdom = rdom.write().unwrap();
            let mut dioxus_state = DioxusState::create(&mut rdom);
            dioxus_state.apply_mutations(&mut rdom, muts);
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

impl Renderer<Arc<EventData>> for DioxusRenderer {
    fn render(&mut self, mut root: dioxus_native_core::NodeMut<()>) {
        let rdom = root.real_dom_mut();
        let muts = self.vdom.render_immediate();
        self.dioxus_state.apply_mutations(rdom, muts);
    }

    fn handle_event(
        &mut self,
        node: dioxus_native_core::NodeMut<()>,
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
                }
            }
        });

        #[cfg(not(all(feature = "hot-reload", debug_assertions)))]
        Box::pin(self.vdom.wait_for_work())
    }
}
