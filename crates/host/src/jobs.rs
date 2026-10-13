use std::ffi::CStr;

use rquickjs::Ctx;

pub const MAX_PENDING_JOBS: usize = 1_000_000;

pub fn run_pending_jobs(ctx: &Ctx<'_>) -> usize {
    let mut jobs = 0;
    while jobs < MAX_PENDING_JOBS {
        if !run_one_job(ctx) {
            break;
        }
        jobs += 1;
    }
    jobs
}

fn run_one_job(ctx: &Ctx<'_>) -> bool {
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

    #[test]
    fn logs_and_continues_after_a_failed_job() {
        let runtime = Runtime::new().unwrap();
        let context = Context::full(&runtime).unwrap();
        context.with(|ctx| {
            ctx.eval::<(), _>(
                b"queueMicrotask(() => { throw new Error('boom'); }); globalThis.done = false; queueMicrotask(() => { globalThis.done = true; });",
            )
            .unwrap();
            let jobs = super::run_pending_jobs(&ctx);
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
            assert_eq!(super::run_pending_jobs(&ctx), 0);
        });
    }
}
