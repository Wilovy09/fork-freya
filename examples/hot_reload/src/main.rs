use freya::prelude::*;
use freya_hot_reload::hot_launch;

// Re-use the app function from lib.rs for the initial render before
// the hot reload dylib is first loaded.
use hot_reload::app;

fn main() {
    hot_launch(
        LaunchConfig::new().with_window(
            WindowConfig::new(app)
                .with_title("Hot Reload Demo")
                .with_size(480., 360.),
        ),
        // Points the watcher to this crate's source directory.
        env!("CARGO_MANIFEST_DIR"),
    );
}
