use std::{
    cell::RefCell,
    rc::Rc,
    sync::atomic::{
        AtomicU64,
        AtomicUsize,
        Ordering,
    },
};

use rustc_hash::FxHashMap;

use crate::{
    prelude::{
        Task,
        TaskId,
    },
    reactive_context::ReactiveContext,
    runner::Message,
    scope::ScopeStorage,
    scope_id::ScopeId,
};

// TODO: rendering flag.
pub struct CurrentContext {
    pub scope_id: ScopeId,
    pub scopes_storages: Rc<RefCell<FxHashMap<ScopeId, ScopeStorage>>>,

    pub tasks: Rc<RefCell<FxHashMap<TaskId, Rc<RefCell<Task>>>>>,
    pub task_id_counter: Rc<AtomicU64>,
    pub sender: futures_channel::mpsc::UnboundedSender<Message>,
}

impl Clone for CurrentContext {
    fn clone(&self) -> Self {
        Self {
            scope_id: self.scope_id,
            scopes_storages: self.scopes_storages.clone(),
            tasks: self.tasks.clone(),
            task_id_counter: self.task_id_counter.clone(),
            sender: self.sender.clone(),
        }
    }
}

/// Getter function exported from the host binary so the dylib can call it
/// explicitly (via a stored `usize` function pointer) to read the host's TLS.
///
/// On macOS executables do not export their symbols to `dlsym` by default, so
/// `dlsym(RTLD_DEFAULT, …)` cannot be used. Instead `hot_launch` passes the
/// address of this function to the dylib via the FFI call arguments. The dylib
/// stores the address in its own `HOST_CTX_GETTER_FN` static and calls it from
/// `with()`.
#[unsafe(no_mangle)]
pub extern "C" fn freya_get_current_context() -> *const CurrentContext {
    CURRENT_CONTEXT.with(|c| {
        c.borrow()
            .as_ref()
            .map(|ctx| ctx as *const CurrentContext)
            .unwrap_or(std::ptr::null())
    })
}

/// Per-image (per-dylib) storage for the host's context getter function pointer.
///
/// Written by `set_host_ctx_getter` (called from within the dylib's
/// `freya_hot_reload_app` expansion) and read by `with()`.  Because both the
/// writer and the reader live in the same binary (the dylib's statically-linked
/// `freya_core`), they share the same physical memory despite TLS isolation.
static HOST_CTX_GETTER_FN: AtomicUsize = AtomicUsize::new(0);

/// Stores the address of the host's `freya_get_current_context` function.
///
/// Called from the dylib's `export_app!` expansion before invoking `$app()`.
/// Overwrites any value from the previous render; no corresponding clear is
/// needed because something in the element builder may call `with()` after the
/// app function returns but before `freya_hot_reload_app` exits.
pub fn set_host_ctx_getter(fn_ptr: usize) {
    HOST_CTX_GETTER_FN.store(fn_ptr, Ordering::Release);
}

impl CurrentContext {
    pub fn run_with_reactive<T>(new_context: Self, run: impl FnOnce() -> T) -> T {
        let reactive_context = CURRENT_CONTEXT.with_borrow_mut(|context| {
            let reactive_context = {
                let scope_storages = new_context.scopes_storages.borrow();
                let scope_storage = scope_storages.get(&new_context.scope_id).unwrap();
                scope_storage.reactive_context.clone()
            };
            context.replace(new_context);
            reactive_context
        });
        let res = ReactiveContext::run(reactive_context, run);
        CURRENT_CONTEXT.with_borrow_mut(|context| context.take());
        res
    }

    pub fn run<T>(new_context: Self, run: impl FnOnce() -> T) -> T {
        CURRENT_CONTEXT.with_borrow_mut(|context| {
            context.replace(new_context);
        });
        let res = run();
        CURRENT_CONTEXT.with_borrow_mut(|context| context.take());
        res
    }

    pub fn with<T>(with: impl FnOnce(&CurrentContext) -> T) -> T {
        // Fast path: TLS is set (normal host-side execution).
        // Check first without moving `with`, then call it on the second access.
        if CURRENT_CONTEXT.with(|c| c.borrow().is_some()) {
            return CURRENT_CONTEXT
                .with(|c| with(c.borrow().as_ref().expect("CurrentContext missing after is_some check")));
        }
        // Fallback for hot-reload dylibs: the host's getter fn was passed via FFI
        // and stored in this binary's HOST_CTX_GETTER_FN static. Calling it reads
        // the host's TLS regardless of per-image isolation.
        let fn_ptr = HOST_CTX_GETTER_FN.load(Ordering::Acquire);
        if fn_ptr != 0 {
            let getter: extern "C" fn() -> *const CurrentContext =
                unsafe { std::mem::transmute(fn_ptr) };
            let ptr = getter();
            if !ptr.is_null() {
                return with(unsafe { &*ptr });
            }
        }
        panic!("Your trying to access Freya's current context outside of it, you might be in a separate thread or async task that is not integrated with Freya.")
    }

    pub fn try_with<T>(with: impl FnOnce(&CurrentContext) -> T) -> Option<T> {
        CURRENT_CONTEXT
            .try_with(|context| {
                if let Ok(context) = context.try_borrow()
                    && let Some(context) = context.as_ref()
                {
                    Some(with(context))
                } else {
                    None
                }
            })
            .ok()
            .flatten()
    }
}

thread_local! {
    static CURRENT_CONTEXT: RefCell<Option<CurrentContext>> = const { RefCell::new(None) }
}
