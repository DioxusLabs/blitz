use dioxus::prelude::*;

#[tokio::main]
async fn main() {
    blitz::launch(app).await;
}

fn app(cx: Scope) -> Element {
    let count = use_state(cx, || 0);

    use_future(cx, (), move |_| {
        let count = count.to_owned();
        let update = cx.schedule_update();
        async move {
            loop {
                count.with_mut(|f| *f += 1);
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                update();
            }
        }
    });

    cx.render(rsx! {
        div {
            width: "100%",
            height: "100%",
            background: "linear-gradient({count}deg, rgb(2,0,36), rgb(186,213,218))",
        }
    })
}
