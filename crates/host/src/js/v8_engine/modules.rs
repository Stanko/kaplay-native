use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use kaplay_native_pak::AssetSource;
use v8::{Global, Local, Module, ModuleStatus, PinScope};

use crate::js::modules::{resolve, MODULE_ORIGIN};

pub struct Modules {
    assets: Rc<AssetSource>,
    cache: RefCell<HashMap<String, Global<Module>>>,
}

pub fn install(isolate: &mut v8::OwnedIsolate, assets: Rc<AssetSource>) {
    isolate.set_slot(Rc::new(Modules {
        assets,
        cache: RefCell::new(HashMap::new()),
    }));
    isolate.set_host_import_module_dynamically_callback(dynamic_import);
    isolate.set_host_initialize_import_meta_object_callback(import_meta);
}

fn loader(scope: &PinScope<'_, '_>) -> Rc<Modules> {
    scope
        .get_slot::<Rc<Modules>>()
        .expect("module loader installed")
        .clone()
}

pub fn compile<'s>(
    scope: &mut PinScope<'s, '_>,
    name: &str,
    source: &str,
) -> Option<Local<'s, Module>> {
    let source = v8::String::new(scope, source)?;
    let resource = v8::String::new(scope, name)?;
    let origin = v8::ScriptOrigin::new(
        scope,
        resource.into(),
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
    let mut source = v8::script_compiler::Source::new(source, Some(&origin));
    let module = v8::script_compiler::compile_module(scope, &mut source)?;
    if let Some(loader) = scope.get_slot::<Rc<Modules>>() {
        loader
            .cache
            .borrow_mut()
            .insert(name.to_string(), Global::new(scope, module));
    }
    Some(module)
}

fn module_name(scope: &PinScope<'_, '_>, module: Local<Module>) -> String {
    loader(scope)
        .cache
        .borrow()
        .iter()
        .find(|(_, cached)| Local::new(scope, *cached) == module)
        .map(|(name, _)| name.clone())
        .unwrap_or_else(|| "game".into())
}

fn fail<'s, T>(scope: &PinScope<'s, '_>, message: &str) -> Option<T> {
    let text = v8::String::new(scope, message)?;
    let error = v8::Exception::type_error(scope, text);
    scope.throw_exception(error);
    None
}

fn load<'s>(
    scope: &mut PinScope<'s, '_>,
    base: &str,
    specifier: &str,
) -> Option<Local<'s, Module>> {
    let name = match resolve(base, specifier) {
        Ok(name) => name,
        Err(error) => return fail(scope, &error),
    };
    let loader = loader(scope);
    if let Some(module) = loader.cache.borrow().get(&name) {
        return Some(Local::new(scope, module));
    }
    let Some(source) = loader.assets.read_text(&name) else {
        return fail(
            scope,
            &format!("cannot load module '{specifier}' from '{base}' (asset '{name}' not found)"),
        );
    };
    compile(scope, &name, &source)
}

pub fn resolve_import<'s>(
    context: Local<'s, v8::Context>,
    specifier: Local<'s, v8::String>,
    _attributes: Local<'s, v8::FixedArray>,
    referrer: Local<'s, Module>,
) -> Option<Local<'s, Module>> {
    v8::callback_scope!(unsafe scope, context);
    let base = module_name(scope, referrer);
    let specifier = specifier.to_rust_string_lossy(scope);
    load(scope, &base, &specifier)
}

extern "C" fn import_meta(
    context: Local<v8::Context>,
    module: Local<Module>,
    meta: Local<v8::Object>,
) {
    v8::callback_scope!(unsafe scope, context);
    let name = module_name(scope, module);
    let key = v8::String::new(scope, "url").unwrap();
    let url = v8::String::new(scope, &format!("{MODULE_ORIGIN}{name}")).unwrap();
    meta.create_data_property(scope, key.into(), url.into());
}

fn dynamic_import<'s>(
    scope: &mut PinScope<'s, '_>,
    _options: Local<'s, v8::Data>,
    resource: Local<'s, v8::Value>,
    specifier: Local<'s, v8::String>,
    _attributes: Local<'s, v8::FixedArray>,
) -> Option<Local<'s, v8::Promise>> {
    let base = resource.to_rust_string_lossy(scope);
    let specifier = specifier.to_rust_string_lossy(scope);
    v8::tc_scope!(let caught, scope);
    if let Some(promise) = import_promise(caught, &base, &specifier) {
        return Some(promise);
    }
    let reason = caught.exception()?;
    caught.reset();
    let resolver = v8::PromiseResolver::new(caught)?;
    resolver.reject(caught, reason);
    Some(resolver.get_promise(caught))
}

fn import_promise<'s>(
    scope: &mut PinScope<'s, '_>,
    base: &str,
    specifier: &str,
) -> Option<Local<'s, v8::Promise>> {
    let module = load(scope, base, specifier)?;
    if module.get_status() == ModuleStatus::Uninstantiated
        && module.instantiate_module(scope, resolve_import) != Some(true)
    {
        return None;
    }
    if module.get_status() == ModuleStatus::Errored {
        let resolver = v8::PromiseResolver::new(scope)?;
        resolver.reject(scope, module.get_exception());
        return Some(resolver.get_promise(scope));
    }
    let result = module.evaluate(scope)?;
    let evaluation = Local::<v8::Promise>::try_from(result).ok()?;
    let fulfilled = v8::Function::builder(
        |_scope: &mut PinScope, args: v8::FunctionCallbackArguments, mut rv: v8::ReturnValue| {
            rv.set(args.data());
        },
    )
    .data(module.get_module_namespace())
    .build(scope)?;
    evaluation.then(scope, fulfilled)
}
