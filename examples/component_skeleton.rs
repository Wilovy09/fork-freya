#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]
use freya::{
    prelude::*,
    skeletons::*,
};

fn main() {
    launch(LaunchConfig::new().with_window(WindowConfig::new(app)))
}

fn app() -> impl IntoElement {
    use_init_root_theme(|| {
        let mut theme = light_theme();
        register_skeleton_themes(&mut theme);
        theme
    });

    let mut loading = use_state(|| true);

    rect()
        .expanded()
        .padding(Gaps::new_all(24.))
        .spacing(12.)
        .child(
            rect().width(Size::fill()).height(Size::px(20.)).child(
                Skeleton::new()
                    .loading(*loading.read())
                    .child("This text appears once loaded"),
            ),
        )
        .child(
            rect().width(Size::px(200.)).height(Size::px(200.)).child(
                Skeleton::new().loading(*loading.read()).child(
                    rect()
                        .expanded()
                        .background((80, 120, 200))
                        .center()
                        .child("This box appears once loaded"),
                ),
            ),
        )
        .child(
            Button::new()
                .child(if *loading.read() {
                    "Mark as loaded"
                } else {
                    "Mark as loading"
                })
                .on_press(move |_| {
                    let current = *loading.read();
                    *loading.write() = !current;
                }),
        )
}
