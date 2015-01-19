use core::prelude::*;
use core::fmt;
use core::fmt::Show;

use opengles::gl2;
use opengles::gl2::GLuint;

use glcommon::{check_gl_error, GLResult, FillDefaults, Defaults};

use log::logi;

use collections::vec::Vec;

#[deriving(PartialEq, Eq, Hash, Show)]
pub enum PixelFormat {
    RGBA = gl2::RGBA as int,
    RGB = gl2::RGB as int,
    ALPHA = gl2::ALPHA as int,
}

pub trait ToPixelFormat {
    fn to_pixelformat(&self) -> GLResult<PixelFormat>;
}

pub struct Texture {
    pub texture: GLuint,
    pub dimensions: (i32, i32),
}

pub struct BrushTexture {
    pub texture: Texture,
    pub source: (PixelFormat, (i32, i32), Vec<u8>),
}

impl Texture {
    pub fn new() -> Texture {
        let texture = gl2::gen_textures(1)[0];
        check_gl_error("gen_textures");
        Texture { texture: texture, dimensions: (0, 0) }
    }
    pub fn with_image(w: i32, h: i32, bytes: Option<&[u8]>, format: PixelFormat) -> Texture {
        let mut texture = Texture::new();
        texture.set_image(w, h, bytes, format);
        texture
    }

    pub fn set_image(&mut self, w: i32, h: i32, bytes: Option<&[u8]>, format: PixelFormat) {
        gl2::bind_texture(gl2::TEXTURE_2D, self.texture);
        check_gl_error("Texture.set_image bind_texture");
        gl2::tex_image_2d(gl2::TEXTURE_2D, 0, format as i32, w, h, 0, format as GLuint, gl2::UNSIGNED_BYTE, bytes);
        check_gl_error("Texture.set_image tex_image_2d");

        gl2::tex_parameter_i(gl2::TEXTURE_2D, gl2::TEXTURE_WRAP_S, gl2::CLAMP_TO_EDGE as i32);
        gl2::tex_parameter_i(gl2::TEXTURE_2D, gl2::TEXTURE_WRAP_T, gl2::CLAMP_TO_EDGE as i32);
        gl2::tex_parameter_i(gl2::TEXTURE_2D, gl2::TEXTURE_MIN_FILTER, gl2::NEAREST as i32);
        gl2::tex_parameter_i(gl2::TEXTURE_2D, gl2::TEXTURE_MAG_FILTER, gl2::NEAREST as i32);
        check_gl_error("Texture.set_image tex_parameter_i");
        self.dimensions = (w,h);
    }
}

impl Drop for Texture {
    fn drop(&mut self) {
        gl2::delete_textures([self.texture].as_slice());
        logi!("deleted {} texture", self.dimensions);
    }
}

impl Show for Texture {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "texture 0x{:x}, dimensions {}", self.texture, self.dimensions)
    }
}

impl Show for BrushTexture {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        write!(formatter, "brushtexture 0x{:x}, dimensions {}", self.texture.texture, self.texture.dimensions)
    }
}

impl FillDefaults<(PixelFormat, (i32, i32), Vec<u8>), (PixelFormat, (i32, i32), Vec<u8>), BrushTexture> for BrushTexture {
    fn fill_defaults(init: (PixelFormat, (i32, i32), Vec<u8>)) -> Defaults<(PixelFormat, (i32, i32), Vec<u8>), BrushTexture> {
        Defaults { val: init }
    }
}
