use std::{collections::HashMap, time::Duration};

use dioxus_native::prelude::*;

pub fn main() {
    dioxus_native::launch(app);
}

fn app() -> Element {
    let mut data = use_store(HashMap::new);
    use_future(move || async move {
        for i in 0..100 {
            tokio::time::sleep(Duration::from_millis(10)).await;
            println!("{i}");
            data.insert(i, i);
        }
    });
    rsx!(
        for (id, _) in data.iter() {
            div {
                key: "item_{id}",
                "{id}"
            }
        }
    )
}
