use dioxus::prelude::*;

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    rsx! {
        h1 { "Bazzica" }
        img { src: "carte/denari_01.svg", width: "120" }
    }
}
