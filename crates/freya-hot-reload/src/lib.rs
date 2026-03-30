//! Hot reload support for Freya applications.
//!
//! Watches your `src/` directory and reloads the UI when files change,
//! keeping the window alive between reloads.
//!
//! # Setup
//!
//! **1. Add a `lib` target to your `Cargo.toml`:**
//! ```toml
//! [lib]
//! crate-type = ["cdylib", "rlib"]
//!
//! [[bin]]
//! name = "my_app"
//! path = "src/main.rs"
//! ```
//!
//! **2. Export your app function in `src/lib.rs`:**
//! ```no_run
//! use freya::prelude::*;
//! use freya_hot_reload::export_app;
//!
//! export_app!(app);
//!
//! fn app() -> impl IntoElement {
//!     rect().child("Hello")
//! }
//! ```
//!
//! **3. Use `hot_launch` instead of `launch` in `src/main.rs`:**
//! ```no_run
//! use freya::prelude::*;
//! use freya_hot_reload::hot_launch;
//!
//! fn main() {
//!     hot_launch(
//!         LaunchConfig::new().with_window(WindowConfig::new(app)),
//!         env!("CARGO_MANIFEST_DIR"),
//!     );
//! }
//!
//! fn app() -> impl IntoElement {
//!     rect().child("Hello")
//! }
//! ```

use std::{
    ffi::c_void,
    path::PathBuf,
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use async_io::Timer;
use blocking::unblock;
use freya_core::{
    current_context::freya_get_current_context,
    element::{
        ComponentProps,
        Element,
        IntoElement,
    },
    reactive_context::freya_get_current_reactive_context,
    scope_id::ScopeId,
};
use freya_winit::{
    config::LaunchConfig,
    renderer::{
        LaunchProxy,
        RendererContext,
    },
};
use futures_channel::mpsc;
use futures_lite::StreamExt;
use libloading::Library;
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// The symbol exported from the user's cdylib.
const SYMBOL: &[u8] = b"freya_hot_reload_app\0";

/// Platform-specific dylib extension.
#[cfg(target_os = "macos")]
const DYLIB_EXT: &str = "dylib";
#[cfg(target_os = "linux")]
const DYLIB_EXT: &str = "so";
#[cfg(target_os = "windows")]
const DYLIB_EXT: &str = "dll";

/// Exported by the user's cdylib.
///
/// Receives the host's getter function pointers as `usize` values so the dylib
/// can call them to read the host's TLS without relying on `dlsym` (which
/// requires the main binary to export symbols dynamically, which it does not by
/// default on macOS).
type HotReloadFn = unsafe extern "C" fn(usize, usize) -> *mut c_void;

/// Launch with hot reload support.
///
/// Watches the `src/` directory of the given crate and hot-reloads the UI on changes.
/// The window stays open between reloads. Logic changes also trigger a reload since
/// the full crate is recompiled -- only the framework itself is never recompiled.
///
/// Pass `env!("CARGO_MANIFEST_DIR")` as the `crate_dir` argument.
pub fn hot_launch(launch_config: LaunchConfig, crate_dir: impl Into<PathBuf>) {
    let crate_dir: Arc<PathBuf> = Arc::new(crate_dir.into());

    // Loaded libraries are intentionally kept alive forever so that vtables
    // pointing into dylib code remain valid for the lifetime of the process.
    let loaded_libs: Arc<std::sync::Mutex<Vec<Library>>> = Arc::default();

    let crate_dir_task = Arc::clone(&crate_dir);
    let loaded_libs_task = Arc::clone(&loaded_libs);

    let launch_config = launch_config.with_future(move |proxy: LaunchProxy| async move {
        let (tx, mut rx) = mpsc::unbounded::<()>();

        let src_dir = crate_dir_task.join("src");

        let mut watcher = RecommendedWatcher::new(
            move |event: notify::Result<notify::Event>| {
                if let Ok(e) = event {
                    if matches!(
                        e.kind,
                        EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
                    ) {
                        let _ = tx.unbounded_send(());
                    }
                }
            },
            Config::default(),
        )
        .expect("Failed to create file watcher");

        watcher
            .watch(&src_dir, RecursiveMode::Recursive)
            .expect("Failed to watch src/");

        loop {
            // Wait for the first file change.
            rx.next().await;

            // Debounce: drain any extra events arriving within 400ms.
            loop {
                let more = futures_lite::future::race(
                    async { rx.next().await.map(|_| true).unwrap_or(false) },
                    async {
                        Timer::after(Duration::from_millis(400)).await;
                        false
                    },
                )
                .await;
                if !more {
                    break;
                }
            }

            eprintln!("[hot-reload] Change detected, rebuilding...");

            let crate_dir_build = Arc::clone(&crate_dir_task);
            let result = unblock(move || {
                std::process::Command::new("cargo")
                    .args(["build", "--lib", "--message-format=json"])
                    .current_dir(crate_dir_build.as_ref())
                    .output()
            })
            .await;

            let dylib_path = match result {
                Ok(output) if output.status.success() => {
                    find_dylib_in_cargo_output(&output.stdout)
                }
                Ok(output) => {
                    eprintln!(
                        "[hot-reload] Build failed:\n{}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                    continue;
                }
                Err(e) => {
                    eprintln!("[hot-reload] Failed to run cargo: {e}");
                    continue;
                }
            };

            let Some(original_path) = dylib_path else {
                eprintln!("[hot-reload] Dylib not found in cargo output");
                continue;
            };

            // Copy to a unique temp path so dlopen loads a fresh version
            // instead of returning the cached handle for the same path.
            let temp_path = std::env::temp_dir().join(format!(
                "freya_hot_reload_{}.{DYLIB_EXT}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            ));

            if let Err(e) = std::fs::copy(&original_path, &temp_path) {
                eprintln!("[hot-reload] Failed to copy dylib: {e}");
                continue;
            }

            let lib = match unsafe { Library::new(&temp_path) } {
                Ok(lib) => lib,
                Err(e) => {
                    eprintln!("[hot-reload] Failed to load dylib: {e}");
                    continue;
                }
            };

            let app_fn: HotReloadFn =
                match unsafe { lib.get::<HotReloadFn>(SYMBOL) } {
                    Ok(sym) => *sym,
                    Err(e) => {
                        eprintln!("[hot-reload] Symbol `freya_hot_reload_app` not found: {e}");
                        eprintln!("[hot-reload] Did you add `export_app!(app)` to your lib.rs?");
                        continue;
                    }
                };

            // Keep the library alive so vtables remain valid.
            loaded_libs_task.lock().unwrap().push(lib);

            // Swap the root component on the renderer thread.
            let swapped = proxy
                .with(move |ctx: &mut RendererContext| {
                    let Some(app_window) = ctx.windows.values_mut().next() else {
                        return false;
                    };
                    let root_scope = app_window
                        .runner
                        .scopes
                        .get(&ScopeId::ROOT)
                        .unwrap();
                    root_scope.borrow_mut().comp =
                        Rc::new(move |_: Rc<dyn ComponentProps>| {
                            // Pass the host's context getter functions as `usize` pointers so
                            // the dylib can call them to access the host's TLS. The dylib cannot
                            // use dlsym because macOS executables do not export symbols
                            // dynamically by default.
                            //
                            // SAFETY: app_fn returns a Box<Element> heap-allocated in the dylib.
                            // Both sides use the same system allocator, and the library is kept
                            // alive for the lifetime of the process.
                            // Cast via fn pointer to avoid "direct cast of function item" warning.
                            let ctx_getter = freya_get_current_context as extern "C" fn() -> *const freya_core::current_context::CurrentContext as usize;
                            let reactive_getter = freya_get_current_reactive_context as extern "C" fn() -> *const freya_core::reactive_context::ReactiveContext as usize;
                            unsafe { *Box::from_raw(app_fn(ctx_getter, reactive_getter) as *mut Element) }
                        });
                    // Reset hook storage so the dylib re-initializes state with its own
                    // type identities rather than failing to downcast values created by
                    // a different binary (TypeId differs across dylib boundaries).
                    app_window.runner.reset_scope_hooks();
                    app_window.runner.invalidate_root();
                    app_window.request_redraw();
                    true
                })
                .await;

            match swapped {
                Ok(true) => eprintln!("[hot-reload] UI reloaded"),
                Ok(false) => eprintln!("[hot-reload] No window to reload"),
                Err(_) => eprintln!("[hot-reload] Renderer channel closed"),
            }
        }
    });

    freya_winit::launch(launch_config);
}

fn find_dylib_in_cargo_output(stdout: &[u8]) -> Option<PathBuf> {
    // Proc-macro crates are also compiled as `.dylib` but they live in
    // `target/debug/deps/` and have a hash suffix (e.g. `libfoo-a1b2c3.dylib`).
    // The user's cdylib goes to `target/debug/libfoo.dylib` – no hash suffix.
    // We look for a dylib that is NOT in a `deps/` subdirectory as a heuristic
    // to skip proc-macro artifacts and find the user's library.
    for line in stdout.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_slice::<serde_json::Value>(line) else {
            continue;
        };
        if msg["reason"].as_str() != Some("compiler-artifact") {
            continue;
        }
        if let Some(filenames) = msg["filenames"].as_array() {
            for f in filenames {
                if let Some(s) = f.as_str() {
                    if s.ends_with(DYLIB_EXT) {
                        let path = PathBuf::from(s);
                        // Skip proc-macro dylibs – they are always in a `deps/` directory.
                        let in_deps = path
                            .parent()
                            .and_then(|p| p.file_name())
                            .map(|name| name == "deps")
                            .unwrap_or(false);
                        if !in_deps {
                            return Some(path);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Internal helpers used by the [`export_app`] macro.
///
/// These re-export from the crate's own copy of `freya_core` so that the
/// macro expansion resolves against the dylib's statically-linked `freya_core`,
/// not the host's.
#[doc(hidden)]
pub mod __private {
    pub use freya_core::current_context::set_host_ctx_getter;
    pub use freya_core::reactive_context::set_host_reactive_getter;
}

/// Internal helper used by the [`export_app`] macro.
#[doc(hidden)]
pub fn __export_element(e: impl IntoElement) -> *mut c_void {
    Box::into_raw(Box::new(e.into_element())) as _
}

/// Exports an app function as the hot reload entry point.
///
/// Place this macro in your `lib.rs` together with the app function.
///
/// # Example
/// ```no_run
/// use freya::prelude::*;
/// use freya_hot_reload::export_app;
///
/// export_app!(app);
///
/// fn app() -> impl IntoElement {
///     rect().child("Hello")
/// }
/// ```
#[macro_export]
macro_rules! export_app {
    ($app:ident) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn freya_hot_reload_app(
            get_ctx: usize,
            get_reactive: usize,
        ) -> *mut ::std::ffi::c_void {
            // The dylib statically links all Rust crates, so every thread-local
            // (CURRENT_CONTEXT, REACTIVE_CONTEXTS_STACK, …) is duplicated and empty.
            // The host passes its getter function pointers as `usize` values. We store
            // them in this dylib's own copy of `freya_core` so that `with()` and
            // `try_current()` can call through them to read the host's TLS.
            $crate::__private::set_host_ctx_getter(get_ctx);
            $crate::__private::set_host_reactive_getter(get_reactive);
            $crate::__export_element($app())
        }
    };
}
