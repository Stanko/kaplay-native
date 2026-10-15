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

mod modules;

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
    log::error!("unhandled rejection: {}", value_error(scope, value));
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
        modules::install(&mut isolate, state.assets.clone());

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

    pub fn eval_module(&self, name: &str, source: &str) -> Result<(), String> {
        let mut isolate = self.isolate.borrow_mut();
        isolate.cancel_terminate_execution();
        v8::scope!(let scope, &mut *isolate);
        let context = Local::new(scope, &self.context);
        let scope = &mut ContextScope::new(scope, context);
        eval_module_in(scope, name, source)
    }

    pub fn eval_preload(&self, source: &str) -> Result<(), String> {
        let mut isolate = self.isolate.borrow_mut();
        isolate.cancel_terminate_execution();
        v8::scope!(let scope, &mut *isolate);
        let context = Local::new(scope, &self.context);
        let scope = &mut ContextScope::new(scope, context);
        eval_preload_in(scope, source)
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
                if message.is_empty() {
                    "JavaScript operation failed without an exception".to_string()
                } else {
                    message
                }
            } else {
                format!("{message}\n{stack}")
            }
        }
    }};
}

fn eval_module_in(
    scope: &mut v8::PinScope<'_, '_>,
    name: &str,
    source: &str,
) -> Result<(), String> {
    v8::tc_scope!(let caught, scope);
    let Some(module) = modules::compile(caught, name, source) else {
        return Err(exception_text!(caught));
    };
    if module.instantiate_module(caught, modules::resolve_import) != Some(true) {
        return Err(exception_text!(caught));
    }
    evaluate_module(caught, module)
}

fn value_error(scope: &mut v8::PinScope<'_, '_>, value: Local<v8::Value>) -> String {
    let message = value.to_rust_string_lossy(scope);
    if let Ok(object) = Local::<v8::Object>::try_from(value) {
        let key = v8::String::new(scope, "stack").unwrap();
        if let Some(stack) = object
            .get(scope, key.into())
            .filter(|stack| stack.is_string())
        {
            let stack = stack.to_rust_string_lossy(scope);
            if !stack.is_empty() {
                return stack;
            }
        }
    }
    message
}

fn evaluate_module(
    scope: &mut v8::PinScope<'_, '_>,
    module: Local<v8::Module>,
) -> Result<(), String> {
    v8::tc_scope!(let caught, scope);
    let Some(result) = module.evaluate(caught) else {
        return Err(exception_text!(caught));
    };
    let promise = Local::<v8::Promise>::try_from(result)
        .map_err(|_| "module evaluation did not return a promise".to_string())?;
    promise.mark_as_handled();
    caught.perform_microtask_checkpoint();
    if promise.state() == v8::PromiseState::Rejected {
        let reason = promise.result(caught);
        return Err(value_error(caught, reason));
    }
    // A pending top-level await can reject after a network/timer callback.
    if promise.state() == v8::PromiseState::Pending {
        let rejected = v8::Function::new(
            caught,
            |scope: &mut v8::PinScope,
             args: v8::FunctionCallbackArguments,
             _rv: v8::ReturnValue| {
                log::error!("module evaluation: {}", value_error(scope, args.get(0)));
            },
        )
        .ok_or_else(|| "failed to install module error handler".to_string())?;
        promise.catch(caught, rejected);
    }
    Ok(())
}

fn eval_preload_in(scope: &mut v8::PinScope<'_, '_>, source: &str) -> Result<(), String> {
    let source = v8::String::new(scope, source)
        .ok_or_else(|| "failed to allocate preload source".to_string())?;
    v8::tc_scope!(let caught, scope);
    if let Some(script) = v8::Script::compile(caught, source, None) {
        script.run(caught);
        if caught.has_caught() {
            return Err(exception_text!(caught));
        }
        caught.perform_microtask_checkpoint();
        return Ok(());
    }
    caught.reset();
    let resource_name = v8::String::new(caught, "preload")
        .ok_or_else(|| "failed to allocate preload name".to_string())?;
    let origin = v8::ScriptOrigin::new(
        caught,
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
    let mut module_source = v8::script_compiler::Source::new(source, Some(&origin));
    let Some(module) = v8::script_compiler::compile_module(caught, &mut module_source) else {
        return Err(exception_text!(caught));
    };
    if module
        .instantiate_module(caught, modules::resolve_import)
        .is_none()
    {
        return Err(exception_text!(caught));
    }
    evaluate_module(caught, module)?;
    expose_module_exports(caught, module)
}

fn expose_module_exports(
    scope: &mut v8::PinScope<'_, '_>,
    module: Local<v8::Module>,
) -> Result<(), String> {
    let namespace = v8::Local::<v8::Object>::try_from(module.get_module_namespace())
        .map_err(|_| "preload module namespace is not an object".to_string())?;
    let names = namespace
        .get_own_property_names(scope, v8::GetPropertyNamesArgs::default())
        .ok_or_else(|| "failed to read preload exports".to_string())?;
    let global = scope.get_current_context().global(scope);
    for index in 0..names.length() {
        let Some(key) = names.get_index(scope, index) else {
            continue;
        };
        let Ok(name) = v8::Local::<v8::String>::try_from(key) else {
            continue;
        };
        if name.to_rust_string_lossy(scope) == "default" {
            continue;
        }
        let Some(value) = namespace.get(scope, key) else {
            continue;
        };
        let _ = global.set(scope, key, value);
    }
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
    use super::{eval_module_in, eval_preload_in, initialize, modules};

    fn with_scope<T>(test: impl FnOnce(&mut v8::PinScope<'_, '_>) -> T) -> T {
        with_assets(&[], test)
    }

    fn with_assets<T>(
        files: &[(&str, &str)],
        test: impl FnOnce(&mut v8::PinScope<'_, '_>) -> T,
    ) -> T {
        initialize();
        let mut isolate = v8::Isolate::new(Default::default());
        isolate.set_microtasks_policy(v8::MicrotasksPolicy::Explicit);
        modules::install(&mut isolate, crate::js::modules::test_assets(files));
        v8::scope!(let scope, &mut isolate);
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);
        test(scope)
    }

    #[test]
    fn module_startup_errors_include_the_message_and_source() {
        with_scope(|scope| {
            let error = eval_module_in(scope, "assets/main.js", "missingKaplay();").unwrap_err();
            assert!(error.contains("missingKaplay"), "{error}");
            assert!(error.contains("assets/main.js"), "{error}");
            let error = eval_module_in(
                scope,
                "assets/await.js",
                "await Promise.reject(new Error('await failed'));",
            )
            .unwrap_err();
            assert!(error.contains("await failed"), "{error}");
        });
    }

    #[test]
    fn resolves_static_dynamic_and_cyclic_imports_from_pak() {
        with_assets(&[
            ("assets/shared.js", "globalThis.loads = (globalThis.loads ?? 0) + 1; export const value = 42; export const url = import.meta.url;"),
            ("assets/a.js", "import { b } from './b.js'; export function a() { return b(); }"),
            ("assets/b.js", "import { a } from './a.js'; export function b() { return 1; }"),
        ], |scope| {
            eval_module_in(scope, "assets/main.js", "import { value, url } from './shared.js'; import { a } from './a.js'; const again = await import('./shared.js'); globalThis.result = [value, again.value, loads, a(), url].join('|');").unwrap();
            assert_eq!(read(scope, "result"), "42|42|1|1|kaplay-native://game/assets/shared.js");
        });
    }

    #[test]
    fn dynamic_import_waits_for_top_level_await() {
        with_assets(&[("assets/lazy.js", "await new Promise(resolve => { globalThis.resumeModule = resolve; }); export const value = 42;")], |scope| {
            eval_module_in(scope, "assets/main.js", "globalThis.result = 0; import('./lazy.js').then(module => { globalThis.result = module.value; });").unwrap();
            assert_eq!(read(scope, "String(result)"), "0");
            read(scope, "resumeModule()");
            scope.perform_microtask_checkpoint();
            assert_eq!(read(scope, "String(result)"), "42");
        });
    }

    #[test]
    fn missing_imports_report_both_paths_and_dynamic_imports_reject() {
        with_scope(|scope| {
            let error =
                eval_module_in(scope, "assets/main.js", "import './missing.js';").unwrap_err();
            assert!(error.contains("assets/main.js"), "{error}");
            assert!(error.contains("assets/missing.js"), "{error}");
            eval_module_in(scope, "assets/dynamic.js", "globalThis.failure = ''; await import('./missing.js').catch(error => { globalThis.failure = error.message; });").unwrap();
            assert!(read(scope, "failure").contains("assets/missing.js"));
        });
    }

    fn read(scope: &mut v8::PinScope<'_, '_>, expression: &str) -> String {
        let source = v8::String::new(scope, expression).unwrap();
        let script = v8::Script::compile(scope, source, None).unwrap();
        script.run(scope).unwrap().to_rust_string_lossy(scope)
    }

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

    #[test]
    fn classic_preloads_define_globals() {
        with_scope(|scope| {
            eval_preload_in(scope, "var fromPreload = 3;").unwrap();
            assert_eq!(read(scope, "String(globalThis.fromPreload)"), "3");
        });
    }

    #[test]
    fn module_preloads_expose_named_exports_as_globals() {
        with_scope(|scope| {
            eval_preload_in(
                scope,
                "const hidden = 4; export const answer = hidden + 38; export default 'ignored';",
            )
            .unwrap();
            assert_eq!(read(scope, "String(globalThis.answer)"), "42");
            assert_eq!(read(scope, "typeof globalThis.hidden"), "undefined");
            assert_eq!(read(scope, "typeof globalThis.default"), "undefined");
        });
    }

    #[test]
    fn preload_errors_do_not_run_the_source_twice() {
        with_scope(|scope| {
            let error = eval_preload_in(
                scope,
                "globalThis.preloadRuns = (globalThis.preloadRuns ?? 0) + 1; throw new Error('boom');",
            )
            .unwrap_err();
            assert!(error.contains("boom"), "{error}");
            assert_eq!(read(scope, "String(globalThis.preloadRuns)"), "1");
        });
    }
}
