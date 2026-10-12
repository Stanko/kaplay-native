use std::rc::Rc;

use kaplay_native_canvas2d::ImageSource;
use kaplay_native_gfx::pixels::UploadOptions;
use rquickjs::function::Rest;
use rquickjs::{Ctx, Function, Object, TypedArray};

use crate::state::HostState;

fn slice_u8<'a, 'js>(array: &'a TypedArray<'js, u8>) -> Option<&'a [u8]> {
    let raw = array.as_raw()?;
    Some(unsafe { std::slice::from_raw_parts(raw.as_ptr() as *const u8, raw.len()) })
}

fn slice_u8_mut<'a, 'js>(array: &'a mut TypedArray<'js, u8>) -> Option<&'a mut [u8]> {
    let raw = array.as_raw()?;
    Some(unsafe { std::slice::from_raw_parts_mut(raw.as_ptr() as *mut u8, raw.len()) })
}

fn slice_f32<'a, 'js>(array: &'a TypedArray<'js, f32>) -> Option<&'a [f32]> {
    let raw = array.as_raw()?;
    Some(unsafe { std::slice::from_raw_parts(raw.as_ptr() as *const f32, raw.len() / 4) })
}

fn handle(value: Option<u32>) -> i32 {
    value.map_or(-1, |value| value as i32)
}

pub fn install<'js>(ctx: &Ctx<'js>, state: Rc<HostState>, shim: &str) -> rquickjs::Result<()> {
    let native = Object::new(ctx.clone())?;
    install_env(ctx, &native, &state)?;
    install_window(ctx, &native, &state)?;
    install_fs(ctx, &native, &state)?;
    install_storage(ctx, &native, &state)?;
    install_image(ctx, &native, &state)?;
    install_font(ctx, &native, &state)?;
    install_canvas(ctx, &native, &state)?;
    install_gl(ctx, &native, &state)?;
    install_steam(ctx, &native, &state)?;
    ctx.globals().set("__kapNative", native)?;
    ctx.eval::<(), _>(shim)?;
    Ok(())
}

fn install_env<'js>(
    ctx: &Ctx<'js>,
    native: &Object<'js>,
    state: &Rc<HostState>,
) -> rquickjs::Result<()> {
    let object = Object::new(ctx.clone())?;
    object.set("hostVersion", state.host_version)?;
    object.set("platform", state.platform)?;
    object.set("gameName", state.game_name.clone())?;
    object.set("networkAllowed", state.allow_network)?;
    {
        let state = state.clone();
        object.set(
            "now",
            Function::new(ctx.clone(), move || -> f64 { state.now_ms() })?,
        )?;
    }
    {
        object.set(
            "log",
            Function::new(
                ctx.clone(),
                move |level: String, message: String| match level.as_str() {
                    "error" => log::error!("{message}"),
                    "warn" => log::warn!("{message}"),
                    _ => log::info!("{message}"),
                },
            )?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "quit",
            Function::new(ctx.clone(), move || {
                state.quit.set(true);
            })?,
        )?;
    }
    native.set("env", object)?;
    Ok(())
}

fn install_window<'js>(
    ctx: &Ctx<'js>,
    native: &Object<'js>,
    state: &Rc<HostState>,
) -> rquickjs::Result<()> {
    let object = Object::new(ctx.clone())?;
    {
        let state = state.clone();
        object.set(
            "size",
            Function::new(ctx.clone(), move || -> Vec<i32> {
                let size = state.window.inner_size();
                vec![size.width as i32, size.height as i32]
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "setTitle",
            Function::new(ctx.clone(), move |title: String| {
                state.window.set_title(&title);
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "setFullscreen",
            Function::new(ctx.clone(), move |enabled: bool| {
                use winit::window::Fullscreen;
                state.fullscreen.set(enabled);
                state.window.set_fullscreen(if enabled {
                    Some(Fullscreen::Borderless(None))
                } else {
                    None
                });
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "isFullscreen",
            Function::new(ctx.clone(), move || -> bool { state.fullscreen.get() })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "setCursor",
            Function::new(ctx.clone(), move |cursor: String| {
                state.set_cursor(&cursor);
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "setCursorLocked",
            Function::new(ctx.clone(), move |locked: bool| {
                state.set_cursor_locked(locked);
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "isCursorLocked",
            Function::new(ctx.clone(), move || -> bool { state.cursor_locked.get() })?,
        )?;
    }
    native.set("window", object)?;
    Ok(())
}

fn install_fs<'js>(
    ctx: &Ctx<'js>,
    native: &Object<'js>,
    state: &Rc<HostState>,
) -> rquickjs::Result<()> {
    let object = Object::new(ctx.clone())?;
    {
        let state = state.clone();
        object.set(
            "size",
            Function::new(ctx.clone(), move |path: String| -> i32 {
                state
                    .assets
                    .read(&path)
                    .map_or(-1, |bytes| bytes.len() as i32)
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "exists",
            Function::new(ctx.clone(), move |path: String| -> bool {
                state.assets.exists(&path)
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "read",
            Function::new(
                ctx.clone(),
                move |path: String, mut out: TypedArray<u8>| -> bool {
                    let Some(bytes) = state.assets.read(&path) else {
                        return false;
                    };
                    let Some(out) = slice_u8_mut(&mut out) else {
                        return false;
                    };
                    let length = bytes.len().min(out.len());
                    out[..length].copy_from_slice(&bytes[..length]);
                    true
                },
            )?,
        )?;
    }
    native.set("fs", object)?;
    Ok(())
}

fn install_storage<'js>(
    ctx: &Ctx<'js>,
    native: &Object<'js>,
    state: &Rc<HostState>,
) -> rquickjs::Result<()> {
    let object = Object::new(ctx.clone())?;
    {
        let state = state.clone();
        object.set(
            "load",
            Function::new(ctx.clone(), move || -> String { state.load_storage() })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "save",
            Function::new(ctx.clone(), move |data: String| {
                state.save_storage(&data);
            })?,
        )?;
    }
    native.set("storage", object)?;
    Ok(())
}

fn install_image<'js>(
    ctx: &Ctx<'js>,
    native: &Object<'js>,
    state: &Rc<HostState>,
) -> rquickjs::Result<()> {
    let object = Object::new(ctx.clone())?;
    {
        let state = state.clone();
        object.set(
            "load",
            Function::new(ctx.clone(), move |url: String| -> i32 {
                let Some(bytes) = state.assets.read(&url) else {
                    return -1;
                };
                handle(state.decode_image(&bytes))
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "decode",
            Function::new(ctx.clone(), move |data: TypedArray<u8>| -> i32 {
                let Some(bytes) = slice_u8(&data) else {
                    return -1;
                };
                handle(state.decode_image(bytes))
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "width",
            Function::new(ctx.clone(), move |id: u32| -> i32 {
                state
                    .images
                    .borrow()
                    .get(&id)
                    .map_or(-1, |image| image.width as i32)
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "height",
            Function::new(ctx.clone(), move |id: u32| -> i32 {
                state
                    .images
                    .borrow()
                    .get(&id)
                    .map_or(-1, |image| image.height as i32)
            })?,
        )?;
    }
    native.set("image", object)?;
    Ok(())
}

fn install_font<'js>(
    ctx: &Ctx<'js>,
    native: &Object<'js>,
    state: &Rc<HostState>,
) -> rquickjs::Result<()> {
    let object = Object::new(ctx.clone())?;
    {
        let state = state.clone();
        object.set(
            "register",
            Function::new(
                ctx.clone(),
                move |family: String, data: TypedArray<u8>| -> bool {
                    let Some(bytes) = slice_u8(&data) else {
                        return false;
                    };
                    state
                        .canvas
                        .borrow_mut()
                        .register_font(&family, bytes.to_vec());
                    true
                },
            )?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "has",
            Function::new(ctx.clone(), move |family: String| -> bool {
                state.canvas.borrow().has_font(&family)
            })?,
        )?;
    }
    native.set("font", object)?;
    Ok(())
}

fn install_canvas<'js>(
    ctx: &Ctx<'js>,
    native: &Object<'js>,
    state: &Rc<HostState>,
) -> rquickjs::Result<()> {
    let object = Object::new(ctx.clone())?;
    {
        let state = state.clone();
        object.set(
            "create",
            Function::new(ctx.clone(), move |width: u32, height: u32| -> i32 {
                state.canvas.borrow_mut().create_canvas(width, height) as i32
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "resize",
            Function::new(ctx.clone(), move |id: u32, width: u32, height: u32| {
                state.canvas.borrow_mut().resize_canvas(id, width, height);
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "destroy",
            Function::new(ctx.clone(), move |id: u32| {
                state.canvas.borrow_mut().destroy_canvas(id);
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "createContext",
            Function::new(ctx.clone(), move |id: u32| -> i32 {
                handle(state.canvas.borrow_mut().create_context(id))
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "setFillStyle",
            Function::new(ctx.clone(), move |context: u32, value: String| {
                state.canvas.borrow_mut().set_fill_style(context, &value);
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "setStrokeStyle",
            Function::new(ctx.clone(), move |context: u32, value: String| {
                state.canvas.borrow_mut().set_stroke_style(context, &value);
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "setFont",
            Function::new(ctx.clone(), move |context: u32, value: String| {
                state.canvas.borrow_mut().set_font(context, &value);
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "setLineWidth",
            Function::new(ctx.clone(), move |context: u32, value: f32| {
                state.canvas.borrow_mut().set_line_width(context, value);
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "setLineJoin",
            Function::new(ctx.clone(), move |context: u32, value: String| {
                state.canvas.borrow_mut().set_line_join(context, &value);
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "setTextBaseline",
            Function::new(ctx.clone(), move |context: u32, value: String| {
                state.canvas.borrow_mut().set_text_baseline(context, &value);
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "setTextAlign",
            Function::new(ctx.clone(), move |context: u32, value: String| {
                state.canvas.borrow_mut().set_text_align(context, &value);
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "clearRect",
            Function::new(
                ctx.clone(),
                move |context: u32, x: f32, y: f32, width: f32, height: f32| {
                    state
                        .canvas
                        .borrow_mut()
                        .clear_rect(context, x, y, width, height);
                },
            )?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "fillText",
            Function::new(
                ctx.clone(),
                move |context: u32, text: String, x: f32, y: f32| {
                    state.canvas.borrow_mut().fill_text(context, &text, x, y);
                },
            )?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "strokeText",
            Function::new(
                ctx.clone(),
                move |context: u32, text: String, x: f32, y: f32| {
                    state.canvas.borrow_mut().stroke_text(context, &text, x, y);
                },
            )?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "measureText",
            Function::new(ctx.clone(), move |context: u32, text: String| -> Vec<f64> {
                let metrics = state.canvas.borrow().measure_text(context, &text);
                vec![
                    metrics.width as f64,
                    metrics.ascent as f64,
                    metrics.descent as f64,
                ]
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "drawImage",
            Function::new(
                ctx.clone(),
                move |context: u32, kind: i32, id: u32, args: Rest<f64>| {
                    let values: Vec<f64> = args.iter().copied().collect();
                    let (sx, sy, sw, sh, dx, dy, dw, dh) = if values.len() >= 8 {
                        (
                            values[0] as f32,
                            values[1] as f32,
                            values[2] as f32,
                            values[3] as f32,
                            values[4] as f32,
                            values[5] as f32,
                            values[6] as f32,
                            values[7] as f32,
                        )
                    } else if values.len() >= 2 {
                        let (width, height) = source_size(&state, kind, id);
                        (
                            0.0,
                            0.0,
                            width as f32,
                            height as f32,
                            values[0] as f32,
                            values[1] as f32,
                            width as f32,
                            height as f32,
                        )
                    } else {
                        return;
                    };
                    let canvas = state.canvas.clone();
                    if kind == 0 {
                        let images = state.images.borrow();
                        let Some(image) = images.get(&id) else {
                            return;
                        };
                        canvas.borrow_mut().draw_image(
                            context,
                            ImageSource {
                                width: image.width,
                                height: image.height,
                                data: &image.pixels,
                            },
                            sx,
                            sy,
                            sw,
                            sh,
                            dx,
                            dy,
                            dw,
                            dh,
                        );
                    } else {
                        canvas
                            .borrow_mut()
                            .draw_image_from_canvas(context, id, sx, sy, sw, sh, dx, dy, dw, dh);
                    }
                },
            )?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "putImageData",
            Function::new(
                ctx.clone(),
                move |context: u32,
                      x: i32,
                      y: i32,
                      width: u32,
                      height: u32,
                      data: TypedArray<u8>| {
                    let Some(bytes) = slice_u8(&data) else {
                        return;
                    };
                    state
                        .canvas
                        .borrow_mut()
                        .put_image_data(context, x, y, width, height, bytes);
                },
            )?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "getImageData",
            Function::new(
                ctx.clone(),
                move |context: u32,
                      x: i32,
                      y: i32,
                      width: u32,
                      height: u32,
                      mut out: TypedArray<u8>| {
                    let data = state
                        .canvas
                        .borrow()
                        .get_image_data(context, x, y, width, height);
                    let Some(out) = slice_u8_mut(&mut out) else {
                        return;
                    };
                    let length = data.len().min(out.len());
                    out[..length].copy_from_slice(&data[..length]);
                },
            )?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "pngLength",
            Function::new(ctx.clone(), move |canvas: u32| -> i32 {
                state
                    .canvas
                    .borrow()
                    .to_png(canvas)
                    .map_or(-1, |data| data.len() as i32)
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "copyPng",
            Function::new(
                ctx.clone(),
                move |canvas: u32, mut out: TypedArray<u8>| -> bool {
                    let Some(data) = state.canvas.borrow().to_png(canvas) else {
                        return false;
                    };
                    let Some(out) = slice_u8_mut(&mut out) else {
                        return false;
                    };
                    let length = data.len().min(out.len());
                    out[..length].copy_from_slice(&data[..length]);
                    true
                },
            )?,
        )?;
    }
    native.set("canvas", object)?;
    Ok(())
}

fn source_size(state: &HostState, kind: i32, id: u32) -> (u32, u32) {
    if kind == 0 {
        state
            .images
            .borrow()
            .get(&id)
            .map_or((0, 0), |image| (image.width, image.height))
    } else {
        state.canvas.borrow().canvas_size(id).unwrap_or((0, 0))
    }
}

fn install_steam<'js>(
    ctx: &Ctx<'js>,
    native: &Object<'js>,
    state: &Rc<HostState>,
) -> rquickjs::Result<()> {
    let object = Object::new(ctx.clone())?;
    {
        let state = state.clone();
        object.set(
            "available",
            Function::new(ctx.clone(), move || -> bool {
                state
                    .steam
                    .borrow()
                    .as_ref()
                    .is_some_and(|api| api.is_initialized())
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "playerName",
            Function::new(ctx.clone(), move || -> Option<String> {
                state
                    .steam
                    .borrow()
                    .as_ref()
                    .and_then(|api| api.player_name())
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "steamId",
            Function::new(ctx.clone(), move || -> String {
                state
                    .steam
                    .borrow()
                    .as_ref()
                    .map(|api| api.steam_id().to_string())
                    .unwrap_or_default()
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "unlockAchievement",
            Function::new(ctx.clone(), move |name: String| -> bool {
                state
                    .steam
                    .borrow()
                    .as_ref()
                    .is_some_and(|api| api.unlock_achievement(&name))
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "isAchievementUnlocked",
            Function::new(ctx.clone(), move |name: String| -> bool {
                state
                    .steam
                    .borrow()
                    .as_ref()
                    .is_some_and(|api| api.is_achievement_unlocked(&name))
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "storeStats",
            Function::new(ctx.clone(), move || -> bool {
                state
                    .steam
                    .borrow()
                    .as_ref()
                    .is_some_and(|api| api.store_stats())
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "setStatInt",
            Function::new(ctx.clone(), move |name: String, value: i32| -> bool {
                state
                    .steam
                    .borrow()
                    .as_ref()
                    .is_some_and(|api| api.set_stat_int(&name, value))
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "getStatInt",
            Function::new(ctx.clone(), move |name: String| -> Option<i32> {
                state
                    .steam
                    .borrow()
                    .as_ref()
                    .and_then(|api| api.get_stat_int(&name))
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "setStatFloat",
            Function::new(ctx.clone(), move |name: String, value: f32| -> bool {
                state
                    .steam
                    .borrow()
                    .as_ref()
                    .is_some_and(|api| api.set_stat_float(&name, value))
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "getStatFloat",
            Function::new(ctx.clone(), move |name: String| -> Option<f32> {
                state
                    .steam
                    .borrow()
                    .as_ref()
                    .and_then(|api| api.get_stat_float(&name))
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "writeCloud",
            Function::new(ctx.clone(), move |name: String, text: String| -> bool {
                state
                    .steam
                    .borrow()
                    .as_ref()
                    .is_some_and(|api| api.write_file(&name, text.as_bytes()))
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "readCloud",
            Function::new(ctx.clone(), move |name: String| -> Option<String> {
                state
                    .steam
                    .borrow()
                    .as_ref()
                    .and_then(|api| api.read_file(&name))
                    .map(|data| String::from_utf8_lossy(&data).into_owned())
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "cloudExists",
            Function::new(ctx.clone(), move |name: String| -> bool {
                state
                    .steam
                    .borrow()
                    .as_ref()
                    .is_some_and(|api| api.file_exists(&name))
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "deleteCloud",
            Function::new(ctx.clone(), move |name: String| -> bool {
                state
                    .steam
                    .borrow()
                    .as_ref()
                    .is_some_and(|api| api.delete_file(&name))
            })?,
        )?;
    }
    {
        let state = state.clone();
        object.set(
            "runCallbacks",
            Function::new(ctx.clone(), move || {
                if let Some(api) = state.steam.borrow().as_ref() {
                    api.run_callbacks();
                }
            })?,
        )?;
    }
    native.set("steam", object)?;
    Ok(())
}

fn install_gl<'js>(
    ctx: &Ctx<'js>,
    native: &Object<'js>,
    state: &Rc<HostState>,
) -> rquickjs::Result<()> {
    let object = Object::new(ctx.clone())?;
    macro_rules! register {
        ($name:literal, $closure:expr) => {
            object.set($name, Function::new(ctx.clone(), $closure)?)?
        };
    }

    {
        let state = state.clone();
        register!("drawingBufferWidth", move || -> i32 {
            state.gl.borrow().drawing_buffer_size().0
        });
    }
    {
        let state = state.clone();
        register!("drawingBufferHeight", move || -> i32 {
            state.gl.borrow().drawing_buffer_size().1
        });
    }
    {
        let state = state.clone();
        register!("resizeDrawingBuffer", move |width: i32, height: i32| {
            state.gl.borrow_mut().resize_drawing_buffer(width, height);
        });
    }
    {
        let state = state.clone();
        register!(
            "viewport",
            move |x: i32, y: i32, width: i32, height: i32| {
                state.gl.borrow_mut().viewport(x, y, width, height);
            }
        );
    }
    {
        let state = state.clone();
        register!("clearColor", move |r: f32, g: f32, b: f32, a: f32| {
            state.gl.borrow_mut().clear_color(r, g, b, a);
        });
    }
    {
        let state = state.clone();
        register!("clear", move |mask: u32| {
            state.gl.borrow_mut().clear(mask);
        });
    }
    {
        let state = state.clone();
        register!("enable", move |capability: u32| {
            state.gl.borrow_mut().enable(capability);
        });
    }
    {
        let state = state.clone();
        register!("disable", move |capability: u32| {
            state.gl.borrow_mut().disable(capability);
        });
    }
    {
        let state = state.clone();
        register!(
            "blendFuncSeparate",
            move |srgb: u32, drgb: u32, salpha: u32, dalpha: u32| {
                state
                    .gl
                    .borrow_mut()
                    .blend_func_separate(srgb, drgb, salpha, dalpha);
            }
        );
    }
    {
        let state = state.clone();
        register!(
            "stencilFunc",
            move |func: u32, reference: i32, mask: u32| {
                state.gl.borrow_mut().stencil_func(func, reference, mask);
            }
        );
    }
    {
        let state = state.clone();
        register!("stencilOp", move |fail: u32, zfail: u32, zpass: u32| {
            state.gl.borrow_mut().stencil_op(fail, zfail, zpass);
        });
    }
    {
        let state = state.clone();
        register!("pixelStorei", move |parameter: u32, value: i32| {
            state.gl.borrow_mut().pixel_store_i32(parameter, value);
        });
    }
    {
        let state = state.clone();
        register!("getParameter", move |parameter: u32| -> i32 {
            state.gl.borrow_mut().get_parameter_i32(parameter)
        });
    }
    {
        let state = state.clone();
        register!("activeTexture", move |unit: u32| {
            state.gl.borrow_mut().active_texture(unit);
        });
    }
    {
        let state = state.clone();
        register!("createTexture", move || -> i32 {
            handle(state.gl.borrow_mut().create_texture())
        });
    }
    {
        let state = state.clone();
        register!("deleteTexture", move |handle: u32| {
            state.gl.borrow_mut().delete_texture(handle);
        });
    }
    {
        let state = state.clone();
        register!("bindTexture", move |target: u32, handle: i32| {
            state
                .gl
                .borrow_mut()
                .bind_texture(target, option_handle(handle));
        });
    }
    {
        let state = state.clone();
        register!(
            "texParameteri",
            move |target: u32, parameter: u32, value: i32| {
                state
                    .gl
                    .borrow_mut()
                    .tex_parameter_i32(target, parameter, value);
            }
        );
    }
    {
        let state = state.clone();
        register!(
            "texImage2D",
            move |target: u32,
                  internal_format: i32,
                  width: i32,
                  height: i32,
                  format: u32,
                  ty: u32,
                  pixels: Option<TypedArray<u8>>| {
                let options = state.gl.borrow().upload_options();
                let owned = pixels
                    .as_ref()
                    .and_then(|array| slice_u8(array))
                    .map(<[u8]>::to_vec);
                state.gl.borrow_mut().tex_image_2d(
                    target,
                    0,
                    internal_format,
                    width,
                    height,
                    0,
                    format,
                    ty,
                    owned.as_deref(),
                    options,
                );
            }
        );
    }
    {
        let state = state.clone();
        register!(
            "texSubImage2D",
            move |target: u32,
                  x: i32,
                  y: i32,
                  width: i32,
                  format: u32,
                  ty: u32,
                  pixels: TypedArray<u8>| {
                let Some(bytes) = slice_u8(&pixels) else {
                    return;
                };
                let bytes = bytes.to_vec();
                let height = if width > 0 {
                    (bytes.len() as i32 / (width * 4)).max(1)
                } else {
                    0
                };
                let options = state.gl.borrow().upload_options();
                state
                    .gl
                    .borrow_mut()
                    .tex_sub_image_2d(target, 0, x, y, width, height, format, ty, &bytes, options);
            }
        );
    }
    {
        let state = state.clone();
        register!(
            "texImage2DImage",
            move |target: u32, internal_format: i32, format: u32, ty: u32, image: u32| {
                let images = state.images.borrow();
                let Some(image) = images.get(&image) else {
                    return;
                };
                let options = state.gl.borrow().upload_options();
                state.gl.borrow_mut().tex_image_2d(
                    target,
                    0,
                    internal_format,
                    image.width as i32,
                    image.height as i32,
                    0,
                    format,
                    ty,
                    Some(&image.pixels),
                    options,
                );
            }
        );
    }
    {
        let state = state.clone();
        register!(
            "texSubImage2DImage",
            move |target: u32, x: i32, y: i32, format: u32, ty: u32, image: u32| {
                let images = state.images.borrow();
                let Some(image) = images.get(&image) else {
                    return;
                };
                let options = state.gl.borrow().upload_options();
                state.gl.borrow_mut().tex_sub_image_2d(
                    target,
                    0,
                    x,
                    y,
                    image.width as i32,
                    image.height as i32,
                    format,
                    ty,
                    &image.pixels,
                    options,
                );
            }
        );
    }
    {
        let state = state.clone();
        register!(
            "texImage2DCanvas",
            move |target: u32, internal_format: i32, format: u32, ty: u32, canvas: u32| {
                let canvases = state.canvas.borrow();
                let Some((width, height)) = canvases.canvas_size(canvas) else {
                    return;
                };
                let Some(bytes) = canvases.canvas_bytes(canvas) else {
                    return;
                };
                let options = UploadOptions {
                    flip_y: state.gl.borrow().upload_options().flip_y,
                    premultiply: false,
                };
                state.gl.borrow_mut().tex_image_2d(
                    target,
                    0,
                    internal_format,
                    width as i32,
                    height as i32,
                    0,
                    format,
                    ty,
                    Some(bytes),
                    options,
                );
            }
        );
    }
    {
        let state = state.clone();
        register!(
            "texSubImage2DCanvas",
            move |target: u32, x: i32, y: i32, format: u32, ty: u32, canvas: u32| {
                let canvases = state.canvas.borrow();
                let Some((width, height)) = canvases.canvas_size(canvas) else {
                    return;
                };
                let Some(bytes) = canvases.canvas_bytes(canvas) else {
                    return;
                };
                let options = UploadOptions {
                    flip_y: state.gl.borrow().upload_options().flip_y,
                    premultiply: false,
                };
                state.gl.borrow_mut().tex_sub_image_2d(
                    target,
                    0,
                    x,
                    y,
                    width as i32,
                    height as i32,
                    format,
                    ty,
                    bytes,
                    options,
                );
            }
        );
    }
    {
        let state = state.clone();
        register!("createBuffer", move || -> i32 {
            handle(state.gl.borrow_mut().create_buffer())
        });
    }
    {
        let state = state.clone();
        register!("deleteBuffer", move |handle: u32| {
            state.gl.borrow_mut().delete_buffer(handle);
        });
    }
    {
        let state = state.clone();
        register!("bindBuffer", move |target: u32, handle: i32| {
            state
                .gl
                .borrow_mut()
                .bind_buffer(target, option_handle(handle));
        });
    }
    {
        let state = state.clone();
        register!(
            "bufferDataSize",
            move |target: u32, size: i32, usage: u32| {
                state.gl.borrow_mut().buffer_data_size(target, size, usage);
            }
        );
    }
    {
        let state = state.clone();
        register!(
            "bufferData",
            move |target: u32, data: TypedArray<u8>, usage: u32| {
                if let Some(bytes) = slice_u8(&data) {
                    state
                        .gl
                        .borrow_mut()
                        .buffer_data_bytes(target, bytes, usage);
                }
            }
        );
    }
    {
        let state = state.clone();
        register!(
            "bufferSubData",
            move |target: u32, offset: i32, data: TypedArray<u8>| {
                if let Some(bytes) = slice_u8(&data) {
                    state.gl.borrow_mut().buffer_sub_data(target, offset, bytes);
                }
            }
        );
    }
    {
        let state = state.clone();
        register!("enableVertexAttribArray", move |index: u32| {
            state.gl.borrow_mut().enable_vertex_attrib_array(index);
        });
    }
    {
        let state = state.clone();
        register!("disableVertexAttribArray", move |index: u32| {
            state.gl.borrow_mut().disable_vertex_attrib_array(index);
        });
    }
    {
        let state = state.clone();
        register!(
            "vertexAttribPointer",
            move |index: u32, size: i32, ty: u32, normalized: bool, stride: i32, offset: i32| {
                state
                    .gl
                    .borrow_mut()
                    .vertex_attrib_pointer(index, size, ty, normalized, stride, offset);
            }
        );
    }
    {
        let state = state.clone();
        register!("drawArrays", move |mode: u32, first: i32, count: i32| {
            state.gl.borrow_mut().draw_arrays(mode, first, count);
        });
    }
    {
        let state = state.clone();
        register!(
            "drawElements",
            move |mode: u32, count: i32, ty: u32, offset: i32| {
                state.gl.borrow_mut().draw_elements(mode, count, ty, offset);
            }
        );
    }
    {
        let state = state.clone();
        register!("createShader", move |kind: u32| -> i32 {
            handle(state.gl.borrow_mut().create_shader(kind))
        });
    }
    {
        let state = state.clone();
        register!("deleteShader", move |handle: u32| {
            state.gl.borrow_mut().delete_shader(handle);
        });
    }
    {
        let state = state.clone();
        register!("shaderSource", move |handle: u32, source: String| -> bool {
            state.gl.borrow_mut().shader_source(handle, &source).is_ok()
        });
    }
    {
        let state = state.clone();
        register!("compileShader", move |handle: u32| {
            state.gl.borrow_mut().compile_shader(handle);
        });
    }
    {
        let state = state.clone();
        register!("getShaderInfoLog", move |handle: u32| -> String {
            state.gl.borrow_mut().get_shader_info_log(handle)
        });
    }
    {
        let state = state.clone();
        register!("createProgram", move || -> i32 {
            handle(state.gl.borrow_mut().create_program())
        });
    }
    {
        let state = state.clone();
        register!("deleteProgram", move |handle: u32| {
            state.gl.borrow_mut().delete_program(handle);
        });
    }
    {
        let state = state.clone();
        register!("attachShader", move |program: u32, shader: u32| {
            state.gl.borrow_mut().attach_shader(program, shader);
        });
    }
    {
        let state = state.clone();
        register!(
            "bindAttribLocation",
            move |program: u32, index: u32, name: String| {
                state
                    .gl
                    .borrow_mut()
                    .bind_attrib_location(program, index, &name);
            }
        );
    }
    {
        let state = state.clone();
        register!("linkProgram", move |handle: u32| {
            state.gl.borrow_mut().link_program(handle);
        });
    }
    {
        let state = state.clone();
        register!("getLinkStatus", move |handle: u32| -> bool {
            state.gl.borrow_mut().get_link_status(handle)
        });
    }
    {
        let state = state.clone();
        register!("getProgramInfoLog", move |handle: u32| -> String {
            state.gl.borrow_mut().get_program_info_log(handle)
        });
    }
    {
        let state = state.clone();
        register!("useProgram", move |handle: i32| {
            state.gl.borrow_mut().use_program(option_handle(handle));
        });
    }
    {
        let state = state.clone();
        register!("getAttribLocation", move |program: u32,
                                             name: String|
              -> i32 {
            state.gl.borrow_mut().get_attrib_location(program, &name)
        });
    }
    {
        let state = state.clone();
        register!("getUniformLocation", move |program: u32,
                                              name: String|
              -> i32 {
            state.gl.borrow_mut().get_uniform_location(program, &name)
        });
    }
    {
        let state = state.clone();
        register!("uniform1f", move |location: i32, value: f32| {
            state.gl.borrow_mut().uniform_1f(location, value);
        });
    }
    {
        let state = state.clone();
        register!("uniform1i", move |location: i32, value: i32| {
            state.gl.borrow_mut().uniform_1i(location, value);
        });
    }
    {
        let state = state.clone();
        register!("uniform2f", move |location: i32, x: f32, y: f32| {
            state.gl.borrow_mut().uniform_2f(location, x, y);
        });
    }
    {
        let state = state.clone();
        register!("uniform3f", move |location: i32, x: f32, y: f32, z: f32| {
            state.gl.borrow_mut().uniform_3f(location, x, y, z);
        });
    }
    {
        let state = state.clone();
        register!(
            "uniform1fv",
            move |location: i32, values: TypedArray<f32>| {
                if let Some(values) = slice_f32(&values) {
                    state.gl.borrow_mut().uniform_1fv(location, values);
                }
            }
        );
    }
    {
        let state = state.clone();
        register!(
            "uniform2fv",
            move |location: i32, values: TypedArray<f32>| {
                if let Some(values) = slice_f32(&values) {
                    state.gl.borrow_mut().uniform_2fv(location, values);
                }
            }
        );
    }
    {
        let state = state.clone();
        register!(
            "uniform3fv",
            move |location: i32, values: TypedArray<f32>| {
                if let Some(values) = slice_f32(&values) {
                    state.gl.borrow_mut().uniform_3fv(location, values);
                }
            }
        );
    }
    {
        let state = state.clone();
        register!(
            "uniformMatrix4fv",
            move |location: i32, values: TypedArray<f32>| {
                if let Some(values) = slice_f32(&values) {
                    state.gl.borrow_mut().uniform_matrix_4fv(location, values);
                }
            }
        );
    }
    {
        let state = state.clone();
        register!("createFramebuffer", move || -> i32 {
            handle(state.gl.borrow_mut().create_framebuffer())
        });
    }
    {
        let state = state.clone();
        register!("deleteFramebuffer", move |handle: u32| {
            state.gl.borrow_mut().delete_framebuffer(handle);
        });
    }
    {
        let state = state.clone();
        register!("bindFramebuffer", move |target: u32, handle: i32| {
            state
                .gl
                .borrow_mut()
                .bind_framebuffer(target, option_handle(handle));
        });
    }
    {
        let state = state.clone();
        register!("createRenderbuffer", move || -> i32 {
            handle(state.gl.borrow_mut().create_renderbuffer())
        });
    }
    {
        let state = state.clone();
        register!("deleteRenderbuffer", move |handle: u32| {
            state.gl.borrow_mut().delete_renderbuffer(handle);
        });
    }
    {
        let state = state.clone();
        register!("bindRenderbuffer", move |target: u32, handle: i32| {
            state
                .gl
                .borrow_mut()
                .bind_renderbuffer(target, option_handle(handle));
        });
    }
    {
        let state = state.clone();
        register!(
            "renderbufferStorage",
            move |target: u32, internal_format: u32, width: i32, height: i32| {
                state
                    .gl
                    .borrow_mut()
                    .renderbuffer_storage(target, internal_format, width, height);
            }
        );
    }
    {
        let state = state.clone();
        register!(
            "framebufferTexture2D",
            move |target: u32, attachment: u32, texture_target: u32, texture: i32, level: i32| {
                state.gl.borrow_mut().framebuffer_texture_2d(
                    target,
                    attachment,
                    texture_target,
                    option_handle(texture),
                    level,
                );
            }
        );
    }
    {
        let state = state.clone();
        register!(
            "framebufferRenderbuffer",
            move |target: u32, attachment: u32, renderbuffer_target: u32, renderbuffer: i32| {
                state.gl.borrow_mut().framebuffer_renderbuffer(
                    target,
                    attachment,
                    renderbuffer_target,
                    option_handle(renderbuffer),
                );
            }
        );
    }
    {
        let state = state.clone();
        register!("checkFramebufferStatus", move |target: u32| -> u32 {
            state.gl.borrow_mut().check_framebuffer_status(target)
        });
    }
    {
        let state = state.clone();
        register!(
            "readPixels",
            move |x: i32, y: i32, width: i32, height: i32, mut out: TypedArray<u8>| {
                let Some(out) = slice_u8_mut(&mut out) else {
                    return;
                };
                state.gl.borrow_mut().read_pixels(x, y, width, height, out);
            }
        );
    }
    {
        let state = state.clone();
        register!("screenshotLength", move || -> i32 {
            let (width, height, pixels) = state.gl.borrow_mut().read_default_pixels_top_down();
            crate::screenshots::encode_bytes(&pixels, width, height)
                .map_or(-1, |data| data.len() as i32)
        });
    }
    {
        let state = state.clone();
        register!("copyScreenshot", move |mut out: TypedArray<u8>| -> bool {
            let (width, height, pixels) = state.gl.borrow_mut().read_default_pixels_top_down();
            let Some(encoded) = crate::screenshots::encode_bytes(&pixels, width, height) else {
                return false;
            };
            let Some(out) = slice_u8_mut(&mut out) else {
                return false;
            };
            let length = encoded.len().min(out.len());
            out[..length].copy_from_slice(&encoded[..length]);
            true
        });
    }
    native.set("gl", object)?;
    Ok(())
}

fn option_handle(handle: i32) -> Option<u32> {
    if handle < 0 {
        None
    } else {
        Some(handle as u32)
    }
}
