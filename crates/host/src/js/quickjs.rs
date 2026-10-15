use std::ffi::{CStr, CString};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use kaplay_native_pak::AssetSource;
use rquickjs::function::Args;
use rquickjs::loader::{ImportAttributes, Loader, Resolver};
use rquickjs::{Context, Function, Runtime};

use super::{JsArg, Watchdog};
use crate::bridge;
use crate::state::HostState;

pub const MAX_PENDING_JOBS: usize = 1_000_000;

pub struct Engine {
    _runtime: Runtime,
    context: Context,
}

impl Engine {
    pub fn new(state: Rc<HostState>, shim: &str, watchdog: Arc<Watchdog>) -> Result<Self, String> {
        let runtime = Runtime::new().map_err(|error| error.to_string())?;
        runtime.set_loader(AssetResolver, AssetLoader(state.assets.clone()));
        install_watchdog(&runtime, watchdog);
        install_rejection_tracker(&runtime);
        let context = Context::full(&runtime).map_err(|error| error.to_string())?;
        context
            .with(|ctx| bridge::quickjs::install(&ctx, state, shim))
            .map_err(|error| error.to_string())?;
        Ok(Self {
            _runtime: runtime,
            context,
        })
    }

    pub fn eval_module(&self, name: &str, source: &str) -> Result<(), String> {
        evaluate_module(&self.context, name, source)
    }

    pub fn eval_preload(&self, source: &str) -> Result<(), String> {
        self.context.with(|ctx| evaluate_preload(&ctx, source))
    }

    pub fn drain_jobs(&self) -> usize {
        self.context.with(|ctx| {
            let jobs = run_pending_jobs(&ctx);
            if jobs >= MAX_PENDING_JOBS {
                log::warn!("stopped after {jobs} pending jobs");
            }
            let pending: rquickjs::Value = ctx.catch();
            if let Some(exception) = pending.as_exception() {
                log::error!(
                    "drain exception: {} :: {}",
                    exception.message().unwrap_or_default(),
                    exception.stack().unwrap_or_default()
                );
            }
            jobs
        })
    }

    pub fn call_global(&self, name: &str, args: &[JsArg<'_>]) -> Result<(), String> {
        self.context.with(|ctx| -> Result<(), String> {
            let function: Function = ctx
                .globals()
                .get(name)
                .map_err(|error| js_error(&ctx, error))?;
            let mut call_args = Args::new(ctx.clone(), args.len());
            for arg in args {
                let pushed = match arg {
                    JsArg::Uint(value) => call_args.push_arg(*value),
                    JsArg::Float(value) => call_args.push_arg(*value),
                    JsArg::Bool(value) => call_args.push_arg(*value),
                    JsArg::Str(value) => call_args.push_arg(*value),
                };
                pushed.map_err(|error| js_error(&ctx, error))?;
            }
            call_args
                .apply::<()>(&function)
                .map_err(|error| js_error(&ctx, error))
        })
    }
}

struct AssetResolver;

impl Resolver for AssetResolver {
    fn resolve<'js>(
        &mut self,
        _ctx: &rquickjs::Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<String> {
        super::modules::resolve(base, name)
            .map_err(|message| rquickjs::Error::new_resolving_message(base, name, message))
    }
}

struct AssetLoader(Rc<AssetSource>);

impl Loader for AssetLoader {
    fn load<'js>(
        &mut self,
        ctx: &rquickjs::Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> rquickjs::Result<rquickjs::Module<'js>> {
        let source = self
            .0
            .read_text(name)
            .ok_or_else(|| rquickjs::Error::new_loading_message(name, "asset not found"))?;
        let module = rquickjs::Module::declare(ctx.clone(), name, source)?;
        module
            .meta()?
            .set("url", format!("{}{name}", super::modules::MODULE_ORIGIN))?;
        Ok(module)
    }
}

fn install_watchdog(runtime: &Runtime, watchdog: Arc<Watchdog>) {
    if std::env::var_os("KAPLAY_NATIVE_WATCHDOG").is_none() {
        return;
    }
    runtime.set_interrupt_handler(Some(Box::new(move || {
        if watchdog.expired() {
            static REPORTED: AtomicBool = AtomicBool::new(false);
            if !REPORTED.swap(true, Ordering::Relaxed) {
                log::error!(
                    "interrupting long-running javascript during {}",
                    watchdog.phase()
                );
            }
            return true;
        }
        false
    })));
}

fn install_rejection_tracker(runtime: &Runtime) {
    runtime.set_host_promise_rejection_tracker(Some(Box::new(
        |_ctx, _promise, reason, handled| {
            if handled {
                return;
            }
            if let Some(exception) = reason.as_exception() {
                log::error!(
                    "unhandled rejection: {} :: {}",
                    exception.message().unwrap_or_default(),
                    exception.stack().unwrap_or_default()
                );
            } else {
                log::error!("unhandled rejection (non-error)");
            }
        },
    )));
}

fn evaluate_script_source(ctx: &rquickjs::Ctx<'_>, source: &str) -> Result<(), String> {
    ctx.eval::<(), _>(source.as_bytes())
        .map_err(|error| js_error(ctx, error))?;
    run_pending_jobs(ctx);
    Ok(())
}

fn evaluate_preload(ctx: &rquickjs::Ctx<'_>, source: &str) -> Result<(), String> {
    if script_compiles(ctx, source) {
        return evaluate_script_source(ctx, source);
    }
    evaluate_module_source(ctx, source)
}

fn script_compiles(ctx: &rquickjs::Ctx<'_>, source: &str) -> bool {
    let Ok(source) = CString::new(source) else {
        return false;
    };
    let flags = (rquickjs::qjs::JS_EVAL_TYPE_GLOBAL
        | rquickjs::qjs::JS_EVAL_FLAG_STRICT
        | rquickjs::qjs::JS_EVAL_FLAG_COMPILE_ONLY) as i32;
    let value = unsafe {
        rquickjs::qjs::JS_Eval(
            ctx.as_raw().as_ptr(),
            source.as_ptr(),
            source.as_bytes().len() as u64,
            c"preload".as_ptr(),
            flags,
        )
    };
    let failed =
        unsafe { rquickjs::qjs::JS_VALUE_GET_TAG(value) == rquickjs::qjs::JS_TAG_EXCEPTION };
    unsafe { rquickjs::qjs::JS_FreeValue(ctx.as_raw().as_ptr(), value) };
    if failed {
        let _ = ctx.catch();
    }
    !failed
}

fn evaluate_module_source(ctx: &rquickjs::Ctx<'_>, source: &str) -> Result<(), String> {
    let module = rquickjs::Module::declare(
        ctx.clone(),
        "preload".to_string(),
        source.as_bytes().to_vec(),
    )
    .map_err(|error| js_error(ctx, error))?;
    let (module, _promise) = module.eval().map_err(|error| js_error(ctx, error))?;
    run_pending_jobs(ctx);
    let namespace = module.namespace().map_err(|error| js_error(ctx, error))?;
    expose_namespace(ctx, &namespace)
}

fn expose_namespace<'js>(
    ctx: &rquickjs::Ctx<'js>,
    namespace: &rquickjs::Object<'js>,
) -> Result<(), String> {
    let globals = ctx.globals();
    for name in namespace.keys::<String>() {
        let name = name.map_err(|error| js_error(ctx, error))?;
        if name == "default" {
            continue;
        }
        let value: rquickjs::Value = namespace
            .get(name.as_str())
            .map_err(|error| js_error(ctx, error))?;
        globals
            .set(name.as_str(), value)
            .map_err(|error| js_error(ctx, error))?;
    }
    Ok(())
}

fn evaluate_module(context: &Context, name: &str, source: &str) -> Result<(), String> {
    context.with(|ctx| {
        let module =
            rquickjs::Module::declare(ctx.clone(), name.to_string(), source.as_bytes().to_vec())
                .map_err(|error| js_error(&ctx, error))?;
        module
            .meta()
            .and_then(|meta| meta.set("url", format!("{}{name}", super::modules::MODULE_ORIGIN)))
            .map_err(|error| js_error(&ctx, error))?;
        let (_, promise) = module.eval().map_err(|error| js_error(&ctx, error))?;
        run_pending_jobs(&ctx);
        if let Some(result) = promise.result::<()>() {
            result.map_err(|error| js_error(&ctx, error))?;
        }
        Ok(())
    })
}

fn js_error(ctx: &rquickjs::Ctx<'_>, error: rquickjs::Error) -> String {
    let pending: rquickjs::Value = ctx.catch();
    if let Some(exception) = pending.as_exception() {
        let message = exception.message().unwrap_or_default();
        let stack = exception.stack().unwrap_or_default();
        if stack.is_empty() {
            return message;
        }
        return format!("{message}\n{stack}");
    }
    error.to_string()
}

pub fn run_pending_jobs(ctx: &rquickjs::Ctx<'_>) -> usize {
    let mut jobs = 0;
    while jobs < MAX_PENDING_JOBS {
        if !run_one_job(ctx) {
            break;
        }
        jobs += 1;
    }
    jobs
}

fn run_one_job(ctx: &rquickjs::Ctx<'_>) -> bool {
    let raw = ctx.as_raw().as_ptr();
    unsafe {
        let runtime = rquickjs::qjs::JS_GetRuntime(raw);
        let mut job_ctx: *mut rquickjs::qjs::JSContext = std::ptr::null_mut();
        let result = rquickjs::qjs::JS_ExecutePendingJob(runtime, &mut job_ctx);
        if result < 0 && !job_ctx.is_null() {
            log_exception(job_ctx);
        }
        result != 0
    }
}

unsafe fn log_exception(ctx: *mut rquickjs::qjs::JSContext) {
    let exception = rquickjs::qjs::JS_GetException(ctx);
    let stack = rquickjs::qjs::JS_GetPropertyStr(ctx, exception, c"stack".as_ptr());
    let text = rquickjs::qjs::JS_ToCString(ctx, stack);
    if text.is_null() {
        let message = rquickjs::qjs::JS_GetPropertyStr(ctx, exception, c"message".as_ptr());
        let pointer = rquickjs::qjs::JS_ToCString(ctx, message);
        if !pointer.is_null() {
            let value = CStr::from_ptr(pointer).to_string_lossy();
            log::error!("uncaught error in job: {value}");
            rquickjs::qjs::JS_FreeCString(ctx, pointer);
        }
        rquickjs::qjs::JS_FreeValue(ctx, message);
    } else {
        let value = CStr::from_ptr(text).to_string_lossy();
        log::error!("uncaught error in job: {value}");
        rquickjs::qjs::JS_FreeCString(ctx, text);
    }
    rquickjs::qjs::JS_FreeValue(ctx, stack);
    rquickjs::qjs::JS_FreeValue(ctx, exception);
}

#[cfg(test)]
mod tests {
    use rquickjs::{Context, Runtime};

    use super::{evaluate_module, evaluate_preload, evaluate_script_source, run_pending_jobs};

    #[test]
    fn logs_and_continues_after_a_failed_job() {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();
        context.with(|ctx| {
            ctx.eval::<(), _>(
                b"queueMicrotask(() => { throw new Error('boom'); }); globalThis.done = false; queueMicrotask(() => { globalThis.done = true; });",
            )
            .unwrap();
            let jobs = run_pending_jobs(&ctx);
            assert_eq!(jobs, 2);
            let done: bool = ctx.eval("globalThis.done").unwrap();
            assert!(done);
            assert!(!ctx.has_exception());
        });
    }

    #[test]
    fn reports_no_jobs_when_queue_is_empty() {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();
        context.with(|ctx| {
            assert_eq!(run_pending_jobs(&ctx), 0);
        });
    }

    #[test]
    fn module_declarations_do_not_clobber_globals() {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();
        context
            .with(|ctx| evaluate_script_source(&ctx, "globalThis.UP = 'global';"))
            .unwrap();
        evaluate_module(
            &context,
            "game",
            "function UP() { return 7; } globalThis.result = UP(); var leaked = 1;",
        )
        .unwrap();
        context.with(|ctx| {
            let kept: String = ctx.eval("globalThis.UP").unwrap();
            let result: i32 = ctx.eval("globalThis.result").unwrap();
            let leaked: String = ctx.eval("typeof globalThis.leaked").unwrap();
            assert_eq!(kept, "global");
            assert_eq!(result, 7);
            assert_eq!(leaked, "undefined");
        });
    }

    #[test]
    fn preload_scripts_still_define_globals() {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();
        context.with(|ctx| {
            evaluate_preload(&ctx, "var fromPreload = 3; globalThis.fromPreload = 4;").unwrap();
            let value: i32 = ctx.eval("globalThis.fromPreload").unwrap();
            assert_eq!(value, 4);
        });
    }

    #[test]
    fn module_preloads_expose_named_exports_as_globals() {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();
        context.with(|ctx| {
            evaluate_preload(
                &ctx,
                "const hidden = 4; export const answer = hidden + 38; export default 'ignored';",
            )
            .unwrap();
            let answer: i32 = ctx.eval("globalThis.answer").unwrap();
            assert_eq!(answer, 42);
            let hidden: String = ctx.eval("typeof globalThis.hidden").unwrap();
            assert_eq!(hidden, "undefined");
            let default_export: String = ctx.eval("typeof globalThis.default").unwrap();
            assert_eq!(default_export, "undefined");
        });
    }

    #[test]
    fn module_preloads_fail_through_the_module_path() {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();
        context.with(|ctx| {
            let error = evaluate_preload(
                &ctx,
                "import { missing } from './missing.js'; export const value = missing;",
            )
            .unwrap_err();
            assert!(error.contains("missing.js"), "{error}");
        });
    }

    #[test]
    fn preload_errors_do_not_run_the_source_twice() {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();
        context.with(|ctx| {
            let error = evaluate_preload(
                &ctx,
                "globalThis.preloadRuns = (globalThis.preloadRuns ?? 0) + 1; throw new Error('boom');",
            )
            .unwrap_err();
            assert!(error.contains("boom"), "{error}");
            let runs: i32 = ctx.eval("globalThis.preloadRuns").unwrap();
            assert_eq!(runs, 1);
        });
    }

    #[test]
    fn web_build_imports_and_startup_errors_are_supported() {
        let runtime = Runtime::new().unwrap();
        runtime.set_loader(super::AssetResolver, super::AssetLoader(crate::js::modules::test_assets(&[
            ("assets/shared.js", "globalThis.loads = (globalThis.loads ?? 0) + 1; export const value = 42; export const url = import.meta.url;"),
        ])));
        let context = Context::full(&runtime).unwrap();
        evaluate_module(&context, "assets/main.js", "import { value, url } from './shared.js'; const again = await import('./shared.js'); globalThis.result = [value, again.value, loads, url].join('|');").unwrap();
        context.with(|ctx| {
            assert_eq!(
                ctx.eval::<String, _>("result").unwrap(),
                "42|42|1|kaplay-native://game/assets/shared.js"
            )
        });
        let error =
            evaluate_module(&context, "bad.js", "throw new Error('startup failed');").unwrap_err();
        assert!(error.contains("startup failed"), "{error}");
    }
}
