#![macro_escape]
use core::prelude::*;
use opengles::gl2;
use opengles::gl2::{GLuint, GLint};
use log::{logi, loge};

fn get_gl_error_name(error: u32) -> &'static str {
    match error {
        gl2::NO_ERROR                      => "GL_NO_ERROR",
        gl2::INVALID_ENUM                  => "GL_INVALID_ENUM",
        gl2::INVALID_VALUE                 => "GL_INVALID_VALUE",
        gl2::INVALID_OPERATION             => "GL_INVALID_OPERATION",
        gl2::INVALID_FRAMEBUFFER_OPERATION => "GL_INVALID_FRAMEBUFFER_OPERATION",
        gl2::OUT_OF_MEMORY                 => "GL_OUT_OF_MEMORY",
        _                                  => "unknown error!",
    }
}

pub fn check_gl_error(name: &str) {
    loop {
        match gl2::get_error() {
            0 => break,
            error => logi!("after {} glError (0x{}): {}\n", name, error, get_gl_error_name(error)),
        }
    }
}

#[allow(dead_code)]
pub fn check_framebuffer_complete() -> bool {
    let (err, result) = match gl2::check_framebuffer_status(gl2::FRAMEBUFFER) {
        gl2::FRAMEBUFFER_COMPLETE => ("FRAMEBUFFER_COMPLETE", true),
        gl2::FRAMEBUFFER_INCOMPLETE_ATTACHMENT => ("FRAMEBUFFER_INCOMPLETE_ATTACHMENT", false),
        gl2::FRAMEBUFFER_INCOMPLETE_MISSING_ATTACHMENT => ("FRAMEBUFFER_INCOMPLETE_MISSING_ATTACHMENT", false),
        gl2::FRAMEBUFFER_INCOMPLETE_DIMENSIONS => ("FRAMEBUFFER_INCOMPLETE_DIMENSIONS", false),
        gl2::FRAMEBUFFER_UNSUPPORTED => ("FRAMEBUFFER_UNSUPPORTED", false),
        _ => ("unknown error!", false)
    };
    logi!("framebuffer status: {}", err);
    result
}

pub fn load_shader(shaderType: gl2::GLenum, source: &str) -> Option<GLuint> {
    let shader = gl2::create_shader(shaderType);
    if shader != 0 {
        gl2::shader_source(shader, [source.as_bytes()].as_slice());
        gl2::compile_shader(shader);
        let compiled = gl2::get_shader_iv(shader, gl2::COMPILE_STATUS);
        if compiled != 0 {
            Some(shader)
        } else {
            let log = gl2::get_shader_info_log(shader);
            loge!("Could not compile shader {}:\n{}\n", shaderType, log);
            gl2::delete_shader(shader);
            None
        }
    } else {
        None
    }
}

pub fn create_program(vertexSource: &str, fragmentSource: &str) -> Option<GLuint> {
    let vert_shader = load_shader(gl2::VERTEX_SHADER, vertexSource);
    let pixel_shader = load_shader(gl2::FRAGMENT_SHADER, fragmentSource);
    let program = gl2::create_program();
    if vert_shader.is_none() || pixel_shader.is_none() || program == 0 {
        return None
    }
    gl2::attach_shader(program, vert_shader.unwrap());
    check_gl_error("glAttachShader");
    gl2::attach_shader(program, pixel_shader.unwrap());
    check_gl_error("glAttachShader");
    gl2::link_program(program);
    if gl2::get_program_iv(program, gl2::LINK_STATUS) as u8 == gl2::TRUE {
        Some(program)
    } else {
        let log = gl2::get_program_info_log(program);
        loge!("Could not link program: \n{}\n", log);
        gl2::delete_program(program);
        None
    }
}

pub fn get_shader_handle(program: GLuint, name: &str) -> Option<GLuint> {
    let handle = gl2::get_attrib_location(program, name);
    logi!("glGetAttribLocation({}) = {}", name, handle);
    check_gl_error(format!("getHandle({})", name).as_slice());
    if handle == -1 { None } else { Some(handle as GLuint) }
}

/// gl silently ignores writes to uniform -1, so this is not strictly necessary
pub fn get_uniform_handle_option(program: GLuint, name: &str) -> Option<GLint> {
    let handle = gl2::get_uniform_location(program, name);
    logi!("glGetUniformLocation({}) = {}", name, handle);
    check_gl_error(format!("getHandle({})", name).as_slice());
    if handle == -1 { None } else { Some(handle) }
}

pub trait Shader {
    fn new(vertopt: Option<&str>, fragopt: Option<&str>) -> Option<Self>;
}

macro_rules! glattrib_f32 (
    // struct elements
    ($handle:expr, $count:expr, $item:ident, $elem:ident) => ({
        // XXX probably also unsafe
        let firstref = unsafe { $item.unsafe_ref(0) };
        gl2::vertex_attrib_pointer_f32($handle, $count, false,
            mem::size_of_val(firstref) as i32,
            // XXX this actually derefences firstref and is completely unsafe
            // is there better way to do offsetof in rust?  there ought to be
            unsafe { mem::transmute(&firstref.$elem) });
        check_gl_error(stringify!(vertex_attrib_pointer($elem)));
        gl2::enable_vertex_attrib_array($handle);
        check_gl_error("enable_vertex_array");
    });
    // densely-packed array
    ($handle:expr, $count:expr, $item:ident) => ({
        let firstref = unsafe { $item.unsafe_ref(0) };
        gl2::vertex_attrib_pointer_f32($handle, $count, false, 0, unsafe { mem::transmute(firstref) });
        check_gl_error(stringify!(vertex_attrib_pointer($handle)));
        gl2::enable_vertex_attrib_array($handle);
        check_gl_error("enable_vertex_array");
    });
)

macro_rules! gl_bindtexture (
    ($texunit:expr, $kind:expr, $texture:expr, $handle:expr) => ({
        gl2::active_texture(gl2::TEXTURE0 + $texunit);
        check_gl_error(stringify!(active_texture($texture)));
        gl2::bind_texture($kind, $texture);
        check_gl_error(stringify!(bind_texture($texture)));
        gl2::uniform_1i($handle, $texunit);
        check_gl_error(stringify!(uniform1i($texture)));
    });
)
