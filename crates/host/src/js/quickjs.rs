use std::ffi::CStr;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rquickjs::function::Args;
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

    pub fn eval_script(&self, source: &str) -> Result<(), String> {
        evaluate_script(&self.context, source)
    }

    pub fn eval_module(&self, name: &str, source: &str) -> Result<(), String> {
        evaluate_module(&self.context, name, source)
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

fn evaluate_script(context: &Context, source: &str) -> Result<(), String> {
    context.with(|ctx| {
        ctx.eval::<(), _>(source.as_bytes())
            .map_err(|error| js_error(&ctx, error))?;
        run_pending_jobs(&ctx);
        Ok(())
    })
}

fn evaluate_module(context: &Context, name: &str, source: &str) -> Result<(), String> {
    context.with(|ctx| {
        let module =
            rquickjs::Module::declare(ctx.clone(), name.to_string(), source.as_bytes().to_vec())
                .map_err(|error| js_error(&ctx, error))?;
        module.eval().map_err(|error| js_error(&ctx, error))?;
        run_pending_jobs(&ctx);
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

    use super::{evaluate_module, evaluate_script, run_pending_jobs};

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
        evaluate_script(&context, "globalThis.UP = 'global';").unwrap();
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
        evaluate_script(&context, "globalThis.fromPreload = 3;").unwrap();
        context.with(|ctx| {
            let value: i32 = ctx.eval("globalThis.fromPreload").unwrap();
            assert_eq!(value, 3);
        });
    }
}
