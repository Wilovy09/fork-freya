use freya::prelude::*;
use freya_hot_reload::export_app;

// This macro exports `app` as the hot reload entry point.
// When files in src/ change, freya-hot-reload recompiles this crate
// and calls this export to replace the running UI without restarting.
export_app!(app);

pub fn app() -> impl IntoElement {
    let mut count = use_state(|| 0);

    rect()
        .expanded()
        .center()
        .direction(Direction::Vertical)
        .spacing(16.)
        .background((30, 30, 30))
        .child(
            rect()
                .padding(Gaps::new_all(24.))
                .corner_radius(CornerRadius::new_all(12.))
                .background((50, 50, 50))
                .direction(Direction::Vertical)
                .center()
                .spacing(12.)
                .child(
                    rect()
                        .direction(Direction::Vertical)
                        .center()
                        .spacing(4.)
                        .child(
                            label()
                                .text("Hot Reload Demo")
                                .font_size(24.)
                                .color(Color::WHITE),
                        )
                        .child(
                            label()
                                .text("Edit src/lib.rs and save to see changes")
                                .font_size(13.)
                                .color((180, 180, 180)),
                        ),
                )
                .child(
                    label()
                        .text(format!("clicked: {}", count()))
                        .font_size(18.)
                        .color((100, 200, 255)),
                )
                .child(
                    Button::new()
                        .on_press(move |_| *count.write() += 1)
                        .child("Click me"),
                ),
        )
}
