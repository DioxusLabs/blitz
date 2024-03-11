// background: rgb(2,0,36);
// background: linear-gradient(90deg, rgba(2,0,36,1) 0%, rgba(9,9,121,1) 35%, rgba(0,212,255,1) 100%);

use dioxus::prelude::{GlobalAttributes, SvgAttributes, *};

fn main() {
    dioxus_blitz::launch(app);
}

fn app() -> Element {
    rsx! {
        head { style { {CSS} } }
        div {
            class: "test", "hi           "
        }
        // div {
        //     display: "flex",
        //     flex_direction: "column",
        //     justify_content: "space-evenly",
        //     height: "400px",
        //     div { class: "subdiv" }
        //     div { class: "subdiv" }
        //     div { class: "subdiv" }
        //     div { class: "subdiv" }
        // }
        div {
            class: "parent",
            dangerous_inner_html: r#"
                <div class="subdiv"></div>
                <div class="subdiv" /></div>
                <div class="subdiv" /></div>
                <div class="subdiv" /></div>
            "#
        }
    }
}

const CSS: &str = r#"
.test {
    border: 5px solid white;
    padding: 10px;
    width: 100px;
    height: 100px;
}

.parent {
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    height: 400px;
}

.subdiv {
    width: 50px;
    height: 50px;
    background-color: #333;
}

"#;
