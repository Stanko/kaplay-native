use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Once};
use std::time::Duration;

use v8::{
    Context, ContextScope, Global, Local, MicrotasksPolicy, OwnedIsolate, PromiseRejectEvent,
    PromiseRejectMessage,
};

use super::{JsArg, Watchdog};
use crate::bridge;
use crate::state::HostState;

static INIT: Once = Once::new();

pub fn version() -> String {
    v8::V8::get_version().to_string()
}

fn initialize() {
    INIT.call_once(|| {
        v8::V8::set_flags_from_string("--expose-gc");
        let platform = v8::new_default_platform(0, false).make_shared();
        v8::V8::initialize_platform(platform);
        v8::V8::initialize();
    });
}

unsafe extern "C" fn promise_rejected(message: PromiseRejectMessage) {
    if message.get_event() != PromiseRejectEvent::PromiseRejectWithNoHandler {
        return;
    }
    let Some(value) = message.get_value() else {
        return;
    };
    let scope = unsafe { v8::CallbackScope::new(&message) };
    let scope = std::pin::pin!(scope);
    let scope = &mut scope.init();
    log::error!("unhandled rejection: {}", value.to_rust_string_lossy(scope));
}

pub struct Engine {
    isolate: RefCell<OwnedIsolate>,
    context: Global<Context>,
    _state: Rc<HostState>,
    watchdog: Option<WatchdogThread>,
}

impl Engine {
    pub fn new(state: Rc<HostState>, shim: &str, watchdog: Arc<Watchdog>) -> Result<Self, String> {
        initialize();
        let mut isolate = v8::Isolate::new(Default::default());
        isolate.set_microtasks_policy(MicrotasksPolicy::Explicit);
        isolate.set_capture_stack_trace_for_uncaught_exceptions(true, 10);
        isolate.set_promise_reject_callback(promise_rejected);

        let context = {
            v8::scope!(let scope, &mut isolate);
            let context = Context::new(scope, Default::default());
            let global = Global::new(&*scope, context);
            let scope = &mut ContextScope::new(scope, context);
            bridge::v8_engine::install(scope, &state, shim)?;
            global
        };

        let watchdog = if std::env::var_os("KAPLAY_NATIVE_WATCHDOG").is_some() {
            Some(WatchdogThread::start(
                isolate.thread_safe_handle(),
                watchdog,
            ))
        } else {
            None
        };

        Ok(Self {
            isolate: RefCell::new(isolate),
            context,
            _state: state,
            watchdog,
        })
    }

    pub fn eval_script(&self, source: &str) -> Result<(), String> {
        let mut isolate = self.isolate.borrow_mut();
        isolate.cancel_terminate_execution();
        v8::scope!(let scope, &mut *isolate);
        let context = Local::new(scope, &self.context);
        let scope = &mut ContextScope::new(scope, context);
        eval_script_in(scope, source)
    }

    pub fn eval_module(&self, name: &str, source: &str) -> Result<(), String> {
        let mut isolate = self.isolate.borrow_mut();
        isolate.cancel_terminate_execution();
        v8::scope!(let scope, &mut *isolate);
        let context = Local::new(scope, &self.context);
        let scope = &mut ContextScope::new(scope, context);
        eval_module_in(scope, name, source)
    }

    pub fn drain_jobs(&self) -> usize {
        let mut isolate = self.isolate.borrow_mut();
        isolate.cancel_terminate_execution();
        v8::scope!(let scope, &mut *isolate);
        let context = Local::new(scope, &self.context);
        let scope = &mut ContextScope::new(scope, context);
        scope.perform_microtask_checkpoint();
        0
    }

    pub fn call_global(&self, name: &str, args: &[JsArg<'_>]) -> Result<(), String> {
        let mut isolate = self.isolate.borrow_mut();
        isolate.cancel_terminate_execution();
        v8::scope!(let scope, &mut *isolate);
        let context = Local::new(scope, &self.context);
        let scope = &mut ContextScope::new(scope, context);
        call_global_in(scope, name, args)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        if let Some(watchdog) = self.watchdog.as_mut() {
            watchdog.stop();
        }
    }
}

macro_rules! exception_text {
    ($caught:expr) => {{
        if $caught.has_terminated() {
            "interrupted".to_string()
        } else {
            let message = $caught
                .exception()
                .map(|value| value.to_rust_string_lossy($caught))
                .unwrap_or_default();
            let stack = $caught
                .stack_trace()
                .map(|value| value.to_rust_string_lossy($caught))
                .unwrap_or_default();
            if stack.is_empty() {
                message
            } else {
                format!("{message}\n{stack}")
            }
        }
    }};
}

fn eval_script_in(scope: &mut v8::PinScope<'_, '_>, source: &str) -> Result<(), String> {
    let source = v8::String::new(scope, source)
        .ok_or_else(|| "failed to allocate script source".to_string())?;
    v8::tc_scope!(let caught, scope);
    if let Some(script) = v8::Script::compile(caught, source, None) {
        script.run(caught);
    }
    if caught.has_caught() {
        return Err(exception_text!(caught));
    }
    caught.perform_microtask_checkpoint();
    Ok(())
}

fn eval_module_in(
    scope: &mut v8::PinScope<'_, '_>,
    name: &str,
    source: &str,
) -> Result<(), String> {
    let source = v8::String::new(scope, source)
        .ok_or_else(|| "failed to allocate module source".to_string())?;
    let resource_name =
        v8::String::new(scope, name).ok_or_else(|| "failed to allocate module name".to_string())?;
    let origin = v8::ScriptOrigin::new(
        scope,
        resource_name.into(),
        0,
        0,
        false,
        -1,
        None,
        false,
        false,
        true,
        None,
    );
    v8::tc_scope!(let caught, scope);
    let mut source = v8::script_compiler::Source::new(source, Some(&origin));
    let Some(module) = v8::script_compiler::compile_module(caught, &mut source) else {
        return Err(exception_text!(caught));
    };
    if module
        .instantiate_module(caught, |_context, _specifier, _attributes, _referrer| None)
        .is_none()
    {
        return Err(exception_text!(caught));
    }
    let _ = module.evaluate(caught);
    if caught.has_caught() {
        return Err(exception_text!(caught));
    }
    caught.perform_microtask_checkpoint();
    Ok(())
}

fn call_global_in(
    scope: &mut v8::PinScope<'_, '_>,
    name: &str,
    args: &[JsArg<'_>],
) -> Result<(), String> {
    let context = scope.get_current_context();
    let global = context.global(scope);
    let key = v8::String::new(scope, name)
        .ok_or_else(|| format!("failed to allocate the name of {name}"))?;
    let value = global
        .get(scope, key.into())
        .ok_or_else(|| format!("{name} is not defined"))?;
    let function =
        Local::<v8::Function>::try_from(value).map_err(|_| format!("{name} is not a function"))?;
    let values: Vec<Local<v8::Value>> = args.iter().map(|arg| js_value(scope, arg)).collect();
    v8::tc_scope!(let caught, scope);
    function.call(caught, global.into(), &values);
    if caught.has_caught() {
        return Err(exception_text!(caught));
    }
    Ok(())
}

fn js_value<'s>(scope: &v8::PinScope<'s, '_>, arg: &JsArg<'_>) -> Local<'s, v8::Value> {
    match arg {
        JsArg::Uint(value) => v8::Integer::new_from_unsigned(scope, *value).into(),
        JsArg::Float(value) => v8::Number::new(scope, *value).into(),
        JsArg::Bool(value) => v8::Boolean::new(scope, *value).into(),
        JsArg::Str(value) => v8::String::new(scope, value)
            .map_or_else(|| v8::String::empty(scope).into(), |value| value.into()),
    }
}

struct WatchdogThread {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl WatchdogThread {
    fn start(isolate: v8::IsolateHandle, watchdog: Arc<Watchdog>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread = std::thread::spawn(move || {
            while !thread_stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(10));
                if !watchdog.expired() {
                    continue;
                }
                static REPORTED: AtomicBool = AtomicBool::new(false);
                if !REPORTED.swap(true, Ordering::Relaxed) {
                    log::error!(
                        "interrupting long-running javascript during {}",
                        watchdog.phase()
                    );
                }
                isolate.terminate_execution();
            }
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }

    fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::initialize;

    #[test]
    fn evaluates_scripts_and_microtasks() {
        initialize();
        let mut isolate = v8::Isolate::new(Default::default());
        v8::scope!(let scope, &mut isolate);
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);
        let source = v8::String::new(
            scope,
            "globalThis.total = 0; Promise.resolve().then(() => { globalThis.total = 41 + 1; });",
        )
        .unwrap();
        let script = v8::Script::compile(scope, source, None).unwrap();
        let _ = script.run(scope);
        scope.perform_microtask_checkpoint();
        let key = v8::String::new(scope, "total").unwrap();
        let global = scope.get_current_context().global(scope);
        let value = global.get(scope, key.into()).unwrap();
        assert_eq!(value.uint32_value(scope), Some(42));
    }
}
