extern crate opengles;
use core::prelude::*;
use core::mem;
use core::ptr;
use alloc::boxed::Box;

use std::c_str::CString;

use log::logi;

use opengles::gl2;
use opengles::gl2::{GLuint, GLenum, GLubyte};

use glcommon::check_gl_error;
use glpoint::draw_path;
use pointshader::PointShader;
use copyshader;
use copyshader::CopyShader;
use gltexture;
use gltexture::Texture;
use matrix;
use eglinit;
use dropfree::DropFree;

use collections::vec::Vec;

static drawIndexes: [GLubyte, ..6] = [
    0, 1, 2,
    0, 2, 3
];

struct TextureTarget {
    framebuffer: GLuint,
    texture: Texture,
}

impl TextureTarget {
    fn new(w: i32, h: i32, format: gltexture::PixelFormat) -> TextureTarget {
        let framebuffer = gl2::gen_framebuffers(1)[0];
        let texture = Texture::with_image(w, h, None, format);

        gl2::bind_framebuffer(gl2::FRAMEBUFFER, framebuffer);
        gl2::framebuffer_texture_2d(gl2::FRAMEBUFFER, gl2::COLOR_ATTACHMENT0, gl2::TEXTURE_2D, texture.texture, 0);
        gl2::clear_color(0f32, 0f32, 0f32, 0f32);
        gl2::clear(gl2::COLOR_BUFFER_BIT);
        TextureTarget { framebuffer: framebuffer, texture: texture }
    }

}

impl Drop for TextureTarget {
    fn drop(&mut self) {
        // should drop texture automatically?
        gl2::delete_frame_buffers([self.framebuffer].as_slice());
        logi!("deleted texturetarget: {} framebuffer {}", self.texture.dimensions, self.framebuffer);
    }
}

/// struct for static storage of data that stays on rust side
struct Data<'a> {
    #[allow(dead_code)]
    dimensions: (i32, i32),
    copy_shader: Option<&'a CopyShader>,
    point_shader: Option<&'a PointShader>,
    anim_shader: Option<&'a CopyShader>,
    targets: [TextureTarget, ..2],
    brush_texture: Texture,
    current_target: uint,
}

/// right now this doesn't require a mutex because it's only used to communicate with GL, which
/// only happens on the single GL thread - EGL is complicated enough without trading off threads and
/// contexts.
// TODO: be future-proof, add one anyway
static mut dataRef: DropFree<Data<'static>> = DropFree(0 as *mut Data);
fn get_safe_data<'a>() -> &'a mut Data<'a> {
    unsafe { mem::transmute(dataRef.get_mut()) }
}

fn print_gl_string(name: &str, s: GLenum) {
    let glstr = gl2::get_string(s);
    logi!("GL {} = {}\n", name, glstr);
}

fn get_current_texturetarget<'a>(data: &'a Data) -> &'a TextureTarget {
    &data.targets[data.current_target]
}

fn get_current_texturesource<'a> (data: &'a Data) -> &'a TextureTarget {
    &data.targets[data.current_target ^ 1]
}

fn get_texturetargets<'a> (data: &'a Data) -> (&'a TextureTarget, &'a TextureTarget) {
    (get_current_texturetarget(data), get_current_texturesource(data))
}

fn perform_copy(destFramebuffer: GLuint, sourceTexture: &Texture, shader: &CopyShader, matrix: &[f32]) -> () {
    gl2::bind_framebuffer(gl2::FRAMEBUFFER, destFramebuffer);
    check_gl_error("bound framebuffer");
    shader.prep(sourceTexture, matrix);
    gl2::draw_elements(gl2::TRIANGLES, drawIndexes.len() as i32, gl2::UNSIGNED_BYTE, Some(drawIndexes.as_slice()));
    check_gl_error("drew elements");
}

#[no_mangle]
pub fn draw_image(w: i32, h: i32, pixels: *const u8) -> () {
    logi("drawing image...");
    let data = get_safe_data();
    let target = get_current_texturetarget(data);
    let (tw, th) = target.texture.dimensions;
    let heightratio = th as f32 / h as f32;
    let widthratio = tw as f32 / w as f32;
    // fit inside
    let ratio = if heightratio > widthratio { heightratio } else { widthratio };
    // account for gl's own scaling
    let (glratiox, glratioy) = (widthratio / ratio, heightratio / ratio);

    let matrix = [ glratiox,                 0f32,                    0f32, 0f32,
                   0f32,                    -glratioy,                0f32, 0f32,
                   0f32,                     0f32,                    1f32, 0f32,
                  (1f32 - glratiox) / 2f32, (1f32 + glratioy) / 2f32, 0f32, 0f32];
    logi!("drawing with ratio: {:5.3f}, glratio {:5.3f}, {:5.3f}, matrix:\n{}", ratio, glratiox, glratioy, matrix::log(matrix.as_slice()));

    unsafe {
        pixels.to_option().map(|x| ::core::slice::raw::buf_as_slice(x, (w*h*4) as uint, |pixelvec| {
            data.copy_shader.map(|shader| {
                let intexture = Texture::with_image(w, h, Some(pixelvec), gltexture::RGBA);
                check_gl_error("creating texture");
                perform_copy(target.framebuffer, &intexture, shader, matrix.as_slice());
            });
        }));
    }
}

#[no_mangle]
pub unsafe fn with_pixels() -> (i32, i32, *const u8) {
    logi("in with_pixels");
    let data = get_safe_data();
    let oldtarget = get_current_texturetarget(data);
    let (x,y) = oldtarget.texture.dimensions;
    let saveshader = CopyShader::new(None, Some(copyshader::noalpha_fragment_shader));
    let pixels = saveshader.map(|shader| {
        let newtarget = TextureTarget::new(x, y, gltexture::RGB);
        let matrix = [1f32,  0f32,  0f32,  0f32,
                      0f32, -1f32,  0f32,  0f32,
                      0f32,  0f32,  1f32,  0f32,
                      0f32,  1f32,  0f32,  1f32,];
        perform_copy(newtarget.framebuffer, &oldtarget.texture, &shader, matrix.as_slice());
        gl2::finish();
        let pixels = gl2::read_pixels(0, 0, x, y, gl2::RGBA, gl2::UNSIGNED_BYTE);
        check_gl_error("read_pixels");
        logi("gl2::read_pixels()");
        pixels
    }).unwrap_or(Vec::new()); // FIXME unsafe
    let pixptr = pixels.as_ptr();
    mem::forget(pixels);
    logi!("returning pixels: {}", pixptr);
    (x, y, pixptr)
}

// TODO: find out how to make C callbacks work and get rid of this
#[no_mangle]
pub unsafe fn release_pixels(vecptr: *mut u8) {
    logi!("releasing pixels: {}", vecptr);
    let pixels = Vec::from_raw_parts(1, 1, vecptr);
    mem::drop(pixels);
}

unsafe fn with_cstr_as_str<T>(ptr: *const i8, callback: |Option<&str>|->T)->T {
    let cstr = ptr.to_option().map_or(None, |b| Some(CString::new(b, false)));
    let vecstr = cstr.as_ref().and_then(|b| b.as_str());
    callback(vecstr)
}

unsafe fn compile_shader<T>(vec: *const i8, frag: *const i8, 
                 constructor: fn (Option<&str>, Option<&str>) -> Option<T>) -> Option<T> {
    // note that this will use the default shader in case of non-utf8 chars
    // also, must be separate lines b/c ownership
    with_cstr_as_str(vec, |vecstr| with_cstr_as_str(frag, |fragstr| {
        constructor(vecstr, fragstr)
    }))
}

#[no_mangle]
pub unsafe fn compile_copy_shader(vec: *const i8, frag: *const i8) -> *const CopyShader {
    let shader = compile_shader(vec, frag, CopyShader::new);
    shader.map(|x| mem::transmute(box x)).unwrap_or(ptr::null())
}

#[no_mangle]
pub unsafe fn compile_point_shader(vec: *const i8, frag: *const i8) -> *const PointShader {
    let shader = compile_shader(vec, frag, PointShader::new);
    shader.map(|x| mem::transmute(box x)).unwrap_or(ptr::null())
}

fn get_safe_shader<'a, T>(shader: *const T) -> Option<&'a T> {
    unsafe { shader.to_option().map(|x| {
        logi!("got shader addr: {:x}", x as *const T as int);
        mem::transmute(x)
    })}
}

// TODO: make an enum for these with a scala counterpart
#[no_mangle]
pub unsafe fn set_copy_shader(shader: *const CopyShader) -> () {
    logi("setting copy shader");
    get_safe_data().copy_shader = get_safe_shader(shader);
}


// these can also be null to unset the shader
// TODO: document better from scala side
#[no_mangle]
pub unsafe fn set_anim_shader(shader: *const CopyShader) -> () {
    logi("setting anim shader");
    get_safe_data().anim_shader = get_safe_shader(shader);
}

#[no_mangle]
pub unsafe fn set_point_shader(shader: *const PointShader) -> () {
    logi("setting point shader");
    get_safe_data().point_shader = get_safe_shader(shader);
}

#[no_mangle]
pub extern fn setup_graphics(w: i32, h: i32) -> bool {
    print_gl_string("Version", gl2::VERSION);
    print_gl_string("Vendor", gl2::VENDOR);
    print_gl_string("Renderer", gl2::RENDERER);
    print_gl_string("Extensions", gl2::EXTENSIONS);

    logi!("setupGraphics({},{})", w, h);
    unsafe {
        let targets = [TextureTarget::new(w, h, gltexture::RGBA), TextureTarget::new(w, h, gltexture::RGBA)];
        dataRef = DropFree::new(Data {
            dimensions: (w, h),
            copy_shader: None,
            point_shader: None,
            anim_shader: None,
            targets: targets,
            brush_texture: Texture::with_image(1, 1, Some([0xffu8].as_slice()), gltexture::ALPHA),
            current_target: 0,
        });
    }

    gl2::viewport(0, 0, w, h);
    gl2::disable(gl2::DEPTH_TEST);
    gl2::blend_func(gl2::SRC_ALPHA, gl2::ONE_MINUS_SRC_ALPHA);
    true
}

#[no_mangle]
pub extern fn draw_queued_points(matrix: *mut f32) {
    match get_safe_data().point_shader {
        Some(point_shader) => {
            let data = get_safe_data();
            gl2::enable(gl2::BLEND);
            gl2::blend_func(gl2::SRC_ALPHA, gl2::ONE_MINUS_SRC_ALPHA);
            let (target, source) = get_texturetargets(data);
            // TODO: brush color selection
            draw_path(target.framebuffer, point_shader, matrix, [1f32, 1f32, 0f32],
                      &data.brush_texture, &source.texture);

            eglinit::egl_swap();
        },
        _ => { }
    }
}

#[no_mangle]
pub extern fn set_brush_texture(w: i32, h: i32, a_pixels: *const u8) {
    logi!("setting brush texture for {:x}", a_pixels as uint);
    let pixelopt = unsafe { a_pixels.to_option() };
    // pixelvec has lifetime of a_pixels, not x
    // there must be some way around this
    let pixelvec: Option<&[u8]> = unsafe {
        pixelopt.map(|x| ::std::slice::raw::buf_as_slice(x, (w*h) as uint, |x| mem::transmute(x)))
    };
    get_safe_data().brush_texture.set_image(w, h, pixelvec, gltexture::ALPHA);
}

#[no_mangle]
pub extern fn clear_buffer() {
    let data = get_safe_data();
    for target in data.targets.iter() {
        gl2::bind_framebuffer(gl2::FRAMEBUFFER, target.framebuffer);
        gl2::clear_color(0f32, 0f32, 0f32, 0f32);
        gl2::clear(gl2::COLOR_BUFFER_BIT);
        check_gl_error("clear framebuffer");
    }
}

#[no_mangle]
pub extern fn render_frame() {
    let data = get_safe_data();
    match (data.copy_shader, data.anim_shader) {
        (Some(copy_shader), Some(anim_shader)) => {
            data.current_target = data.current_target ^ 1;
            let copymatrix = matrix::identity.as_slice();
            gl2::disable(gl2::BLEND);
            let (target, source) = get_texturetargets(data);
            perform_copy(target.framebuffer, &source.texture, anim_shader, copymatrix);
            perform_copy(0 as GLuint, &target.texture, copy_shader, copymatrix);
        },
        _ => { },
    }
}

#[no_mangle]
pub extern fn deinit_gl() {
    unsafe { dataRef.destroy(); }
    gl2::finish();
}

#[no_mangle]
pub unsafe extern fn deinit_copy_shader(s: *const CopyShader) {
    let s: Box<CopyShader> = mem::transmute(s);
    mem::drop(s);
}

#[no_mangle]
pub unsafe extern fn deinit_point_shader(s: *const PointShader) {
    let s: Box<PointShader> = mem::transmute(s);
    mem::drop(s);
}
