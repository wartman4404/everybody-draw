extern crate opengles;
use core::prelude::*;
use core::mem;
use core::iter;
use collections::vec::Vec;
use core::borrow::IntoCow;

use opengles::gl2;
use opengles::gl2::{GLuint, GLenum, GLubyte};

use glcommon::{check_gl_error, GLResult};
use glpoint::{MotionEventConsumer};
use point::ShaderPaintPoint;
use pointshader::PointShader;
use paintlayer::{TextureTarget, CompletedLayer};
use copyshader::*;
use gltexture::{Texture, PixelFormat};
use matrix;
use eglinit;
use luascript::LuaScript;
use paintlayer::PaintLayer;
use lua_callbacks::{LuaCallbackType};
use lua_geom::{do_interpolate_lua, finish_lua_script};
use drawevent::Events;
use rustjni::JNICallbackClosure;


static DRAW_INDEXES: [GLubyte; 6] = [
    0, 1, 2,
    0, 2, 3
];

const UNDO_BUFFERS: i32 = 5;

//#[deriving(FromPrimitive)]

/// struct for storage of data that stays on rust side
/// should probably be given a meaningful name like PaintContext, but w/e
pub struct GLInit<'a> {
    #[allow(dead_code)]
    pub dimensions: (i32, i32),
    pub paintstate: PaintState<'a>,
    targetdata: TargetData,
    pub points: Vec<Vec<ShaderPaintPoint>>,
    undo_shader: &'a CopyShader,
}

pub struct TargetData {
    targets: [TextureTarget; 2],
    current_target: u8,
}

pub struct UndoTargets {
    targets: [TextureTarget; UNDO_BUFFERS as usize],
    start: i32,
    len: i32,
    max: i32,
    pos: i32,
}

impl UndoTargets {
    pub fn new() -> UndoTargets {
        UndoTargets {
            targets: unsafe { mem::uninitialized() }, // backing array for ringbuffer
            start: 0, // offset of first index in ringbuffer
            max: 0, // highest allocated index in target (anything above this is uninitialized)
            len: 0, // length of ringbuffer
            pos: 0, // index after most recent position in ringbuffer (first index that will be cleared by a push)
        }
    }
    #[inline(always)] #[allow(dead_code)]
    pub fn len(&self) -> i32 { self.len }

    #[inline(always)]
    fn get_pos(&self, pos: i32) -> i32 { (self.start + pos) % UNDO_BUFFERS }

    pub fn push_new_buffer(&mut self, buf: &TextureTarget, copyshader: &CopyShader) {
        let end = self.get_pos(self.pos);
        let target = &mut self.targets[end as usize];
        if end >= self.max {
            let (x, y) = buf.texture.dimensions;
            *target = TextureTarget::new(x, y, PixelFormat::RGBA);
            self.max += 1;
        }
        gl2::bind_framebuffer(gl2::FRAMEBUFFER, target.framebuffer);
        gl2::blend_func(gl2::ONE, gl2::ZERO);
        perform_copy(target.framebuffer, &buf.texture, copyshader, matrix::IDENTITY.as_slice());
        if self.pos < UNDO_BUFFERS {
            self.pos += 1;
        } else {
            let next = self.start + 1;
            self.start = if next == UNDO_BUFFERS { 0 } else { next };
        }
        self.len = self.pos;
    }

    pub fn load_buffer_at(&mut self, idx: i32, buf: &TextureTarget, copyshader: &CopyShader) {
        if idx >= self.len {
            loge!("undo index {} exceeds current buffer size {}!", idx, self.len);
            return;
        }
        debug_logi!("loading undo buffer {}/{}", idx, self.len);
        self.pos = idx + 1;
        let src = &mut self.targets[self.get_pos(idx) as usize];
        gl2::bind_framebuffer(gl2::FRAMEBUFFER, buf.framebuffer);
        gl2::blend_func(gl2::ONE, gl2::ZERO);
        perform_copy(buf.framebuffer, &src.texture, copyshader, matrix::IDENTITY.as_slice());
    }

    pub fn clear_buffers(&mut self) {
        self.start = self.pos;
        self.len = 0;
        self.pos = 0;
    }
}

#[unsafe_destructor]
impl Drop for UndoTargets {
    fn drop(&mut self) -> () {
        for pos in range(0, self.max) {
            mem::drop(&mut self.targets[self.get_pos(pos) as usize]);
        }
    }
}

pub struct PaintState<'a> {
    pub pointshader: Option<&'a PointShader>,
    pub animshader: Option<&'a CopyShader>,
    pub copyshader: Option<&'a CopyShader>,
    pub brush: Option<&'a Texture>,
    pub interpolator: Option<&'a LuaScript>,
    pub layers: Vec<PaintLayer<'a>>,
    pub undo_targets: UndoTargets,
    pub brush_color: [f32; 3],
    pub brush_size: f32,
}

impl<'a> PaintState<'a> {
    pub fn new() -> PaintState<'a> {
        PaintState {
            pointshader: None,
            animshader: None,
            copyshader: None,
            brush: None,
            interpolator: None,
            layers: Vec::new(),
            undo_targets: UndoTargets::new(),
            brush_color: [1f32, 1f32, 0f32],
            brush_size: 1f32,
        }
    }
}

fn print_gl_string(name: &str, s: GLenum) {
    let glstr = gl2::get_string(s);
    debug_logi!("GL {} = {}\n", name, glstr);
}

fn perform_copy(dest_framebuffer: GLuint, source_texture: &Texture, shader: &CopyShader, matrix: &[f32]) -> () {
    gl2::bind_framebuffer(gl2::FRAMEBUFFER, dest_framebuffer);
    check_gl_error("bound framebuffer");
    shader.prep(source_texture, matrix);
    gl2::draw_elements(gl2::TRIANGLES, DRAW_INDEXES.len() as i32, gl2::UNSIGNED_BYTE, Some(DRAW_INDEXES.as_slice()));
    check_gl_error("drew elements");
}

fn draw_layer(layer: CompletedLayer, matrix: &[f32], color: [f32; 3], size: f32
              , brush: &Texture, back_buffer: &Texture, points: &[ShaderPaintPoint]) {
    if points.len() > 0 {
        gl2::bind_framebuffer(gl2::FRAMEBUFFER, layer.target.framebuffer);
        layer.pointshader.prep(matrix.as_slice(), points, color, size, brush, back_buffer);
        gl2::draw_arrays(gl2::POINTS, 0, points.len() as i32);
        check_gl_error("draw_arrays");
    }
}

impl TargetData {
    fn get_current_texturetarget<'a>(&'a self) -> &'a TextureTarget {
        &self.targets[self.current_target as usize]
    }

    fn get_current_texturesource<'a> (&'a self) -> &'a TextureTarget {
        &self.targets[(self.current_target ^ 1) as usize]
    }

    fn get_texturetargets<'a> (&'a self) -> (&'a TextureTarget, &'a TextureTarget) {
        (self.get_current_texturetarget(), self.get_current_texturesource())
    }
}

impl<'a> GLInit<'a> {
    pub fn draw_image(&mut self, w: i32, h: i32, pixels: &[u8], rotation: matrix::Rotation) -> () {
        let target = self.targetdata.get_current_texturetarget();
        let (tw, th) = target.texture.dimensions;
        let heightratio = th as f32 / h as f32;
        let widthratio = tw as f32 / w as f32;
        // fit inside
        let ratio = if heightratio > widthratio { heightratio } else { widthratio };
        // account for gl's own scaling
        let (glratiox, glratioy) = (widthratio / ratio, heightratio / ratio);

        let matrix = matrix::fit_inside((w, h), target.texture.dimensions, rotation);
        debug_logi!("drawing image with ratio: {:5.3}, glratio {:5.3}, {:5.3}", ratio, glratiox, glratioy);

        let intexture = Texture::with_image(w, h, Some(pixels), PixelFormat::RGBA);
        check_gl_error("creating texture");
        perform_copy(target.framebuffer, &intexture, self.undo_shader, matrix.as_slice());
    }

    pub fn get_buffer_dimensions(&self) -> (i32, i32) {
        self.targetdata.get_current_texturetarget().texture.dimensions
    }

    pub fn get_pixels(&mut self, pixels: &mut [u8]) {
        let oldtarget = self.targetdata.get_current_texturetarget();
        let (x,y) = oldtarget.texture.dimensions;
        gl2::bind_framebuffer(gl2::FRAMEBUFFER, oldtarget.framebuffer);
        check_gl_error("read_pixels");
        // The only purpose of the shader copy is to flip the image from gl coords to bitmap coords.
        // it might be better to finagle the output copy matrix so the rest of the targets
        // can stay in bitmap coords?  Or have a dedicated target for this.
        let saveshader = ::glstore::init_from_defaults((None, Some(include_str!("../includes/shaders/noalpha_copy.fsh").into_cow()))).unwrap();
        let newtarget = TextureTarget::new(x, y, PixelFormat::RGB);
        let matrix = [1f32,  0f32,  0f32,  0f32,
                      0f32, -1f32,  0f32,  0f32,
                      0f32,  0f32,  1f32,  0f32,
                      0f32,  1f32,  0f32,  1f32,];
        perform_copy(newtarget.framebuffer, &oldtarget.texture, &saveshader, matrix.as_slice());
        gl2::finish();
        gl2::read_pixels_into(0, 0, x, y, gl2::RGBA, gl2::UNSIGNED_BYTE, pixels);
        check_gl_error("read_pixels");
    }

    // TODO: make an enum for these with a scala counterpart
    pub fn set_copy_shader(&mut self, shader: &'a CopyShader) -> () {
        debug_logi!("setting copy shader");
        self.paintstate.copyshader = Some(shader);
    }

    // these can also be null to unset the shader
    // TODO: document better from scala side
    pub fn set_anim_shader(&mut self, shader: &'a CopyShader) -> () {
        debug_logi!("setting anim shader");
        self.paintstate.animshader = Some(shader);
    }

    pub fn set_point_shader(&mut self, shader: &'a PointShader) -> () {
        debug_logi!("setting point shader");
        self.paintstate.pointshader = Some(shader);
    }

    pub fn set_interpolator(&mut self, interpolator: &'a LuaScript) -> () {
        debug_logi!("setting interpolator");
        self.paintstate.interpolator = Some(interpolator);
    }

    pub fn set_brush_texture(&mut self, texture: &'a Texture) {
        self.paintstate.brush = Some(texture);
    }

    pub fn set_brush_size(&mut self, size: f32) {
        self.paintstate.brush_size = size;
    }

    pub fn set_brush_color(&mut self, color: i32) {
        self.paintstate.brush_color[0] = (((color & 0x00ff0000) >> 16) as f32) / 255f32;
        self.paintstate.brush_color[1] = (((color & 0x0000ff00) >> 8) as f32) / 255f32;
        self.paintstate.brush_color[2] = (((color & 0x000000ff) >> 0) as f32) / 255f32;
    }

    pub fn add_layer(&mut self, layer: PaintLayer<'a>) -> () {
        debug_logi!("adding layer");
        let extra: i32 = (layer.pointidx as i32 + 1) - self.points.len() as i32;
        if extra > 0 {
            //self.points.extend(iter::repeat(Vec::new()).take(extra as usize));
            self.points.extend(iter::range(0, extra).map(|_| Vec::new()));
        }
        self.paintstate.layers.push(layer);
    }

    pub fn clear_layers(&mut self) {
        debug_logi!("setting layer count to 0");
        self.paintstate.layers.clear();
        self.points.truncate(1);
    }

    #[inline]
    pub fn erase_layer(&mut self, layer: i32) -> GLResult<()> {
        let target = match layer {
            0 => self.targetdata.get_current_texturetarget().framebuffer,
            _ => match self.paintstate.layers.as_slice().get((layer - 1) as usize) {
                Some(layer) => layer.target.framebuffer,
                None => return Err(format!("tried to erase layer {} of {}", layer - 1, self.paintstate.layers.len()).into_cow()),
            },
        };
        gl2::bind_framebuffer(gl2::FRAMEBUFFER, target);
        gl2::clear_color(0f32, 0f32, 0f32, 0f32);
        gl2::clear(gl2::COLOR_BUFFER_BIT);
        Ok(())
    }

    pub fn setup_graphics(w: i32, h: i32, events: &mut Events<'a>) -> GLInit<'a> {
        print_gl_string("Version", gl2::VERSION);
        print_gl_string("Vendor", gl2::VENDOR);
        print_gl_string("Renderer", gl2::RENDERER);
        print_gl_string("Extensions", gl2::EXTENSIONS);

        debug_logi!("setupGraphics({},{})", w, h);
        let targets = [TextureTarget::new(w, h, PixelFormat::RGBA), TextureTarget::new(w, h, PixelFormat::RGBA)];
        let mut points: Vec<Vec<ShaderPaintPoint>> = Vec::new();

        // yuck!
        let outputshaderidx = events.load_copyshader(None, None).unwrap();
        let outputshader = events.use_copyshader(outputshaderidx).unwrap();

        let mut paintstate = PaintState::new();
        paintstate.copyshader = Some(outputshader);

        points.push(Vec::new());
        let data = GLInit {
            dimensions: (w, h),
            targetdata: TargetData {
                targets: targets,
                current_target: 0,
            },
            points: points,
            paintstate: paintstate,
            undo_shader: outputshader,
        };

        gl2::viewport(0, 0, w, h);
        gl2::disable(gl2::DEPTH_TEST);
        gl2::blend_func(gl2::ONE, gl2::ONE_MINUS_SRC_ALPHA);

        data
    }

    pub fn unload_interpolator(&mut self, handler: &mut MotionEventConsumer, events: &'a mut Events<'a>, undo_callback: &JNICallbackClosure) -> GLResult<()> {
        if let Some(interpolator) = self.paintstate.interpolator {
            debug_logi!("finishing {:?}", interpolator);
            unsafe {
                let mut callback = LuaCallbackType::new(self, events, handler, undo_callback);
                finish_lua_script(&mut callback, interpolator)
            }
        } else {
            Ok(())
        }
    }

    pub fn push_undo_frame(&mut self) -> i32 {
        let source = self.targetdata.get_current_texturetarget(); // should be identical when called from within lua callback
        self.paintstate.undo_targets.push_new_buffer(source, self.undo_shader);
        self.paintstate.undo_targets.len
    }

    pub fn load_undo_frame(&mut self, idx: i32) {
        let source = self.targetdata.get_current_texturetarget();
        self.paintstate.undo_targets.load_buffer_at(idx, source, self.undo_shader);
    }

    pub fn clear_undo_frames(&mut self) {
        self.paintstate.undo_targets.clear_buffers();
    }

    pub fn draw_queued_points(&mut self, handler: &mut MotionEventConsumer, events: &'a mut Events<'a>, matrix: &matrix::Matrix, undo_callback: &JNICallbackClosure) -> GLResult<()> {
        match (self.paintstate.pointshader, self.paintstate.copyshader, self.paintstate.brush) {
            (Some(point_shader), Some(copy_shader), Some(brush)) => {
                let interp_error = match self.paintstate.interpolator {
                    Some(interpolator) => unsafe {
                        let mut callback = LuaCallbackType::new(self, events, handler, undo_callback);
                        do_interpolate_lua(interpolator, &mut callback)
                    },
                    None => Ok(())
                };
                // lua calls like push_undo_frame may have changed the blend fn
                // TODO: would it be better to use noalpha_copy for them?
                // probably not, sub-base layers might still happen
                gl2::enable(gl2::BLEND);
                gl2::blend_func(gl2::ONE, gl2::ONE_MINUS_SRC_ALPHA);

                let (target, source) = self.targetdata.get_texturetargets();

                let back_buffer = &source.texture;
                let drawvecs = self.points.as_mut_slice();
                let matrix = matrix.as_slice();
                let color = self.paintstate.brush_color;
                let size = self.paintstate.brush_size;
                let baselayer = CompletedLayer {
                    copyshader: copy_shader,
                    pointshader: point_shader,
                    target: target,
                };
                draw_layer(baselayer, matrix, color, size, brush, back_buffer, drawvecs[0].as_slice());

                for layer in self.paintstate.layers.iter() {
                    let completed = layer.complete(copy_shader, point_shader);
                    let points = drawvecs[layer.pointidx as usize].as_slice();
                    draw_layer(completed, matrix, color, size, brush, back_buffer, points);
                }

                for drawvec in drawvecs.iter_mut() {
                    drawvec.clear();
                }

                interp_error
            },
            _ => { Ok(()) }
        }
    }

    pub fn copy_layers_down(&mut self) {
        if let (Some(copy_shader), Some(point_shader)) = (self.paintstate.copyshader, self.paintstate.pointshader) {
            let copymatrix = matrix::IDENTITY.as_slice();
            let target = self.targetdata.get_current_texturetarget();
            gl2::enable(gl2::BLEND);
            gl2::blend_func(gl2::ONE, gl2::ONE_MINUS_SRC_ALPHA);
            for layer in self.paintstate.layers.iter() {
                let completed = layer.complete(copy_shader, point_shader);
                perform_copy(target.framebuffer, &layer.target.texture, completed.copyshader, copymatrix);
                gl2::bind_framebuffer(gl2::FRAMEBUFFER, layer.target.framebuffer);
                gl2::clear_color(0f32, 0f32, 0f32, 0f32);
                gl2::clear(gl2::COLOR_BUFFER_BIT);
                debug_logi!("copied brush layer down");
            }
        }
    }

    pub fn clear_buffer(&mut self) {
        for target in self.targetdata.targets.iter() {
            gl2::bind_framebuffer(gl2::FRAMEBUFFER, target.framebuffer);
            gl2::clear_color(0f32, 0f32, 0f32, 0f32);
            gl2::clear(gl2::COLOR_BUFFER_BIT);
            check_gl_error("clear framebuffer");
        }
    }

    pub fn render_frame(&mut self) {
        match (self.paintstate.copyshader, self.paintstate.animshader) {
            (Some(copy_shader), Some(anim_shader)) => {
                self.targetdata.current_target = self.targetdata.current_target ^ 1;
                let copymatrix = matrix::IDENTITY.as_slice();
                gl2::disable(gl2::BLEND);
                let (target, source) = self.targetdata.get_texturetargets();
                perform_copy(target.framebuffer, &source.texture, anim_shader, copymatrix);
                perform_copy(0 as GLuint, &target.texture, copy_shader, copymatrix);
                gl2::enable(gl2::BLEND);
                for layer in self.paintstate.layers.iter() {
                    perform_copy(0 as GLuint, &layer.target.texture, layer.copyshader.unwrap_or(copy_shader), copymatrix);
                }
                eglinit::egl_swap();
            },
            (x, y) => {
                debug_logi!("skipped frame! copyshader is {:?}, animshader is {:?}", x, y);
            }
        }
    }

    pub unsafe fn destroy(&mut self) {
        gl2::finish();
    }
}

#[allow(dead_code, unused_must_use)]
fn test_all() {
    {
        //let mut data = GLInit::setup_graphics(0, 0);
        //let (mut consumer, producer) = create_motion_event_handler();
        //let copyshader = data.compile_copy_shader(None, None).unwrap();
        //let pointshader = data.compile_point_shader(None, None).unwrap();
        //let interpolator = data.compile_luascript(None).unwrap();
        //let brushpixels = [1u8, 0, 0, 1];
        //let brush = data.load_texture(1, 1, brushpixels, unsafe { mem::transmute(ANDROID_BITMAP_FORMAT_A_8) }).unwrap();

        //data.set_copy_shader(copyshader);
        //data.set_anim_shader(copyshader);
        //data.set_point_shader(pointshader);
        //data.set_interpolator(interpolator);
        //data.set_brush_texture(brush);
        //data.clear_layers();
        //data.add_layer(copyshader, pointshader, 0);
        //data.draw_image(1, 1, brushpixels);
        ////let point = ShaderPaintPoint { pos: ::point::Coordinate { x: 0f32, y: 0f32 }, time: 0f32, size: 0f32, speed: ::point::Coordinate { x: 0f32, y: 0f32 }, distance: 0f32, counter: 0f32 };
        ////data.points[0].push(point);
        
        //data.clear_buffer();
        //let result = data.draw_queued_points(&mut *consumer, &matrix::IDENTITY);
        //match result {
            //Err(err) => logi!("error drawing points: {}", err),
            //Ok(_)    => logi!("drew points successfully)"),
        //};
        //data.render_frame();
        //unsafe {
            //destroy_motion_event_handler(consumer, producer);
        //}
    }
}
