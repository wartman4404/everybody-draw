use core::prelude::*;
use core::mem;
use alloc::boxed::Box;
use jni::{jobject, jclass, jmethodID, JNIEnv, jint, jfloat, jboolean, jfloatArray, JNINativeMethod};

use glcommon::GLResult;
use log::logi;
use glinit::GLInit;
use drawevent::event_stream::EventStream;
use rustjni::android_bitmap::AndroidBitmap;
use drawevent::Events;
use matrix::Matrix;

use rustjni::{register_classmethods, CaseClass, get_safe_data, str_to_jstring, GLInitEvents, JNIUndoCallback, JNICallbackClosure};
use jni_helpers::ToJValue;
use jni_constants::*;


static mut LUA_EXCEPTION: CaseClass = CaseClass { constructor: 0 as jmethodID, class: 0 as jclass };

impl<'a> ::core::ops::Fn<(i32,), ()> for JNICallbackClosure<'a> {
    extern "rust-call" fn call(&self, args: (i32,)) -> () {
        let (arg,) = args;
        unsafe {
            self.undo_callback.call(self.env, arg);
        }
    }
}

unsafe fn rethrow_lua_result(env: *mut JNIEnv, result: GLResult<()>) {
    if let Err(msg) = result {
        let errmsg = str_to_jstring(env, msg.as_slice()).as_jvalue();
        let err = LUA_EXCEPTION.construct(env, [errmsg].as_mut_slice());
        ((**env).Throw)(env, err);
    }
}

unsafe extern "C" fn init_gl(env: *mut JNIEnv, _: jobject, w: jint, h: jint, callback: jobject) -> jint {
    mem::transmute(box GLInitEvents {
        glinit: GLInit::setup_graphics(w, h),
        events: Events::new(),
        jni_undo_callback: JNIUndoCallback::new(env, callback),
    })
}

unsafe extern "C" fn finish_gl(env: *mut JNIEnv, _: jobject, data: jint) {
    let mut data: Box<GLInitEvents> = mem::transmute(data);
    data.jni_undo_callback.destroy(env);
    data.glinit.destroy();
    logi!("finished deinit");
}

unsafe extern "C" fn native_draw_queued_points(env: *mut JNIEnv, _: jobject, data: i32, handler: i32, java_matrix: jfloatArray) {
    let data = get_safe_data(data);
    let callback = data.jni_undo_callback.create_closure(env);
    let mut matrix: Matrix = mem::uninitialized();
    ((**env).GetFloatArrayRegion)(env, java_matrix, 0, 16, matrix.as_mut_ptr());
    let luaerr = data.glinit.draw_queued_points(mem::transmute(handler), &mut data.events, &matrix, &callback);
    rethrow_lua_result(env, luaerr);
}

unsafe extern "C" fn native_finish_lua_script(env: *mut JNIEnv, _: jobject, data: i32, handler: i32) {
    let data = get_safe_data(data);
    let callback = data.jni_undo_callback.create_closure(env);
    let luaerr = data.glinit.unload_interpolator(mem::transmute(handler), &mut data.events, &callback);
    rethrow_lua_result(env, luaerr);
}

unsafe extern "C" fn native_update_gl(_: *mut JNIEnv, _: jobject, data: i32) {
    let data = get_safe_data(data);
    data.glinit.render_frame();
    data.events.pushframe(); // FIXME make sure a frame was actually drawn! No java exceptions, missing copy shader, etc
}

unsafe extern "C" fn set_anim_shader(_: *mut JNIEnv, _: jobject, data: jint, shader: jint) {
    let data = get_safe_data(data);
    let shader = data.events.use_animshader(mem::transmute(shader));
    data.glinit.set_anim_shader(shader);
}

unsafe extern "C" fn set_copy_shader(_: *mut JNIEnv, _: jobject, data: jint, shader: jint) {
    let data = get_safe_data(data);
    let shader = data.events.use_copyshader(mem::transmute(shader));
    data.glinit.set_copy_shader(shader);
}

unsafe extern "C" fn set_point_shader(_: *mut JNIEnv, _: jobject, data: jint, shader: jint) {
    let data = get_safe_data(data);
    let shader = data.events.use_pointshader(mem::transmute(shader));
    data.glinit.set_point_shader(shader);
}

unsafe extern "C" fn set_brush_texture(_: *mut JNIEnv, _: jobject, data: jint, texture: jint) {
    let data = get_safe_data(data);
    let brush = data.events.use_brush(mem::transmute(texture));
    data.glinit.set_brush_texture(&brush.texture);
}

unsafe extern "C" fn clear_framebuffer(_: *mut JNIEnv, _: jobject, data: jint) {
    let data = get_safe_data(data);
    data.events.clear();
    data.glinit.clear_buffer();
}

pub unsafe extern "C" fn export_pixels(env: *mut JNIEnv, _: jobject, data: i32) -> jobject {
    let glinit = &mut get_safe_data(data).glinit;
    let (w, h) = glinit.get_buffer_dimensions();
    let mut bitmap = AndroidBitmap::new(env, w, h);
    glinit.get_pixels(bitmap.as_mut_slice());
    bitmap.set_premultiplied(true);
    bitmap.obj
}

pub unsafe extern "C" fn draw_image(env: *mut JNIEnv, _: jobject, data: i32, bitmap: jobject) {
    // TODO: ensure rgba_8888 format and throw error
    let bitmap = AndroidBitmap::from_jobject(env, bitmap);
    let pixels = bitmap.as_slice();
    get_safe_data(data).glinit.draw_image(bitmap.info.width as i32, bitmap.info.height as i32, pixels);
}

unsafe extern "C" fn jni_lua_set_interpolator(_: *mut JNIEnv, _: jobject, data: jint, scriptid: jint) {
    let data = get_safe_data(data);
    let script = data.events.use_interpolator(mem::transmute(scriptid));
    data.glinit.set_interpolator(script);
}

unsafe extern "C" fn jni_add_layer(_: *mut JNIEnv, _: jobject, data: jint, copyshader: jint, pointshader: jint, pointidx: jint) {
    let data = get_safe_data(data);
    let copyshader = Some(mem::transmute(copyshader));
    let pointshader = Some(mem::transmute(pointshader));
    let layer = data.events.add_layer(data.glinit.dimensions, copyshader, pointshader, mem::transmute(pointidx));
    data.glinit.add_layer(layer);
}

unsafe extern "C" fn jni_clear_layers(_: *mut JNIEnv, _: jobject, data: jint) {
    let data = get_safe_data(data);
    data.events.clear_layers();
    data.glinit.clear_layers();
}

unsafe extern "C" fn jni_replay_begin(_: *mut JNIEnv, _: jobject, data: jint) -> jint {
    let data = get_safe_data(data);
    data.glinit.clear_layers();
    data.glinit.clear_buffer();
    mem::transmute(box EventStream::new())
}

#[allow(unused)]
unsafe extern "C" fn jni_replay_advance_frame(env: *mut JNIEnv, _: jobject, data: jint, replay: jint, java_matrix: jfloatArray) -> jboolean {
    let data = get_safe_data(data);
    let replay: &mut EventStream = mem::transmute(replay);
    let mut matrix: Matrix = mem::uninitialized();
    ((**env).GetFloatArrayRegion)(env, java_matrix, 0, 16, matrix.as_mut_ptr());
    let done = replay.advance_frame(&mut data.glinit, &mut data.events);
    let callback = data.jni_undo_callback.create_closure(env);
    data.glinit.draw_queued_points(&mut replay.consumer, &mut data.events, &matrix, &callback);
    if done { JNI_TRUE as jboolean } else { JNI_FALSE as jboolean }
}

unsafe extern "C" fn jni_replay_destroy(_: *mut JNIEnv, _: jobject, replay: jint) {
    let replay: Box<EventStream> = mem::transmute(replay);
    mem::drop(replay);
}

unsafe extern "C" fn jni_load_undo(_: *mut JNIEnv, _: jobject, data: jint, idx: jint) {
    let data = get_safe_data(data);
    data.glinit.load_undo_frame(idx);
}

unsafe extern "C" fn jni_set_brush_color(_: *mut JNIEnv, _: jobject, data: jint, color: jint) {
    get_safe_data(data).glinit.set_brush_color(color);
}

unsafe extern "C" fn jni_set_brush_size(_: *mut JNIEnv, _: jobject, data: jint, size: jfloat) {
    get_safe_data(data).glinit.set_brush_size(size);
}

pub unsafe fn init(env: *mut JNIEnv) {
    LUA_EXCEPTION = CaseClass::new(env, cstr!("com/github/wartman4404/gldraw/LuaException"), cstr!("(Ljava/lang/String;)V")); 

    let glinitstaticmethods = [
        native_method!("initGL", "(IILcom/github/wartman4404/gldraw/UndoCallback;)I", init_gl),
        native_method!("destroy", "(I)V", finish_gl),
    ];
    register_classmethods(env, cstr!("com/github/wartman4404/gldraw/GLInit$"), &glinitstaticmethods);

    let texturemethods = [
        native_method!("nativeUpdateGL", "(I)V", native_update_gl),
        native_method!("nativeDrawQueuedPoints", "(II[F)V", native_draw_queued_points),
        native_method!("nativeFinishLuaScript", "(II)V", native_finish_lua_script),
        native_method!("nativeClearFramebuffer", "(I)V", clear_framebuffer),
        native_method!("drawImage", "(ILandroid/graphics/Bitmap;)V", draw_image),
        native_method!("nativeSetAnimShader", "(II)Z", set_anim_shader),
        native_method!("nativeSetCopyShader", "(II)Z", set_copy_shader),
        native_method!("nativeSetPointShader", "(II)Z", set_point_shader),
        native_method!("nativeSetBrushTexture", "(II)V", set_brush_texture),
        native_method!("exportPixels", "(I)Landroid/graphics/Bitmap;", export_pixels),
        native_method!("nativeSetInterpolator", "(II)V", jni_lua_set_interpolator),
        native_method!("nativeAddLayer", "(IIII)V", jni_add_layer),
        native_method!("nativeClearLayers", "(I)V", jni_clear_layers),
        native_method!("nativeLoadUndo", "(II)V", jni_load_undo),
        native_method!("nativeSetBrushColor", "(II)V", jni_set_brush_color),
        native_method!("nativeSetBrushSize", "(IF)V", jni_set_brush_size),
    ];
    register_classmethods(env, cstr!("com/github/wartman4404/gldraw/TextureSurfaceThread"), &texturemethods);
    logi!("registered texture thread methods!");

    let replayhandlerstaticmethods = [
        native_method!("init", "(I)I", jni_replay_begin),
        native_method!("destroy", "(I)V", jni_replay_destroy),
        native_method!("advanceFrame", "(II[F)Z", jni_replay_advance_frame),
    ];
    register_classmethods(env, cstr!("com/github/wartman4404/gldraw/Replay$"), &replayhandlerstaticmethods);
    logi!("registered replay methods!");
}

pub unsafe fn destroy(env: *mut JNIEnv) {
    LUA_EXCEPTION.destroy(env);
}
