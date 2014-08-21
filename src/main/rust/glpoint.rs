//extern crate core;
use core::prelude::*;
use core::mem;
use core::slice;

use std::sync::{Once, ONCE_INIT, spsc_queue};

use collections::vec::Vec;
use collections::SmallIntMap;
use collections::MutableMap;
use collections::MutableSeq;
use collections::Mutable;
use collections::Map;

use log::logi;
use motionevent::append_motion_event;
use android::input::AInputEvent;

use opengles::gl2::*;

use pointshader::{PointShader};
use glcommon::check_gl_error;
use gltexture::Texture;
use point;
use point::{ShaderPaintPoint, Coordinate, PointEntry, PointConsumer, PointProducer};
use rollingaverage::RollingAverage;
use dropfree::DropFree;
use matrix::Matrix;

/// lifetime storage for a pointer's past state
struct PointStorage {
    info: Option<ShaderPaintPoint>,
    sizeavg: RollingAverage<f32>,
    speedavg: RollingAverage<f32>,
}

struct RustStatics {
    consumer: PointConsumer,
    producer: PointProducer,
    currentPoints: SmallIntMap<PointStorage>,
    drawvec: Vec<ShaderPaintPoint>,
    pointCounter: i32,
}

static mut dataRef: DropFree<RustStatics> = DropFree(0 as *mut RustStatics);
static mut pathinit: Once = ONCE_INIT;

fn do_path_init() -> () {
    unsafe {
        pathinit.doit(|| {
            let (consumer, producer) = spsc_queue::queue::<PointEntry>(0);
            dataRef = DropFree::new(RustStatics {
                consumer: consumer,
                producer: producer,
                currentPoints: SmallIntMap::new(),
                drawvec: Vec::new(),
                pointCounter: 0,
            });
            logi("created statics");
        });
    }
}

fn get_statics<'a>() -> &'a mut RustStatics {
    do_path_init();
    unsafe { dataRef.get_mut() }
}

#[no_mangle]
//FIXME: needs meaningful name
pub extern fn jni_append_motion_event(evt: *const AInputEvent) {
    let s = get_statics();
    append_motion_event(evt, &mut s.producer);
}

fn manhattan_distance(a: Coordinate, b: Coordinate) -> f32 {
    let x = if a.x > b.x { a.x - b.x } else { b.x - a.x };
    let y = if a.y > b.y { a.y - b.y } else { b.y - a.y };
    return if x > y { x } else { y };
}

#[allow(dead_code)]
fn append_points(a: ShaderPaintPoint, b: ShaderPaintPoint, c: &mut Vec<ShaderPaintPoint>, count: uint) -> () {
    // transform seconds from [0..timescale] to [0..1]
    // this is done here to avoid rollover resulting in negative steptime
    // it might be better to leave it alone and do fract() in the vertex shader?
    let timescale = 10f32;
    let stepx = (b.pos.x - a.pos.x) / count as f32;
    let stepy = (b.pos.y - a.pos.y) / count as f32;
    let steptime = (b.time - a.time) / (count as f32 * timescale);
    let stepsize = (b.size - a.size) / count as f32;
    let stepspeed = (b.speed - a.speed) / count as f32;
    let stepdistance = (b.distance - a.distance) / count as f32;
    let mut addPoint = a;
    addPoint.time = (addPoint.time / timescale) % 1f32;
    for _ in range(0, count) {
        c.push(addPoint);
        addPoint.pos.x += stepx;
        addPoint.pos.y += stepy;
        addPoint.time += steptime;
        addPoint.time = if addPoint.time > 1f32 { addPoint.time - 1f32 } else { addPoint.time };
        addPoint.size += stepsize;
        addPoint.speed += stepspeed;
        addPoint.distance += stepdistance;
    }
}

pub fn draw_path(framebuffer: GLuint, shader: &PointShader, matrix: *mut f32, color: [f32, ..3], brush: &Texture, backBuffer: &Texture) -> () {
    let s = get_statics();
    let ref mut queue = s.consumer;
    let ref mut currentPoints = s.currentPoints;

    let ref mut pointvec = s.drawvec;
    pointvec.clear();

    let mut pointCounter = s.pointCounter;

    loop {
        match queue.pop() {
            Some(point) => {
                let idx = point.index;
                let newpoint = point.entry;
                if !currentPoints.contains_key(&(idx as uint)) {
                    currentPoints.insert(idx as uint, PointStorage {
                        info: None,
                        sizeavg: RollingAverage::new(16),
                        speedavg: RollingAverage::new(16),
                    });
                }
                let oldpoint = currentPoints.find_mut(&(idx as uint)).unwrap();
                match (oldpoint.info, newpoint) {
                    (Some(op), point::Point(np)) => {
                        let dist = manhattan_distance(op.pos, np.pos);
                        let avgsize = oldpoint.sizeavg.push(np.size);
                        let avgspeed = oldpoint.speedavg.push(dist);
                        let npdata = ShaderPaintPoint {
                            pos: np.pos,
                            time: np.time,
                            size: avgsize,
                            speed: avgspeed,
                            distance: op.distance + dist,
                            counter: op.counter,
                        };
                        interpolate_lua_from_rust(op, npdata, backBuffer.dimensions, pointvec);
                        oldpoint.info = Some(npdata);
                    },
                    (_, point::Stop) => {
                        oldpoint.info = None;
                    },
                    (_, point::Point(p)) => {
                        let oldCounter = pointCounter;
                        pointCounter += 1;
                        oldpoint.info = Some(ShaderPaintPoint {
                            pos: p.pos,
                            time: p.time,
                            size: p.size,
                            distance: 0f32,
                            speed: 0f32,
                            counter: oldCounter as f32,
                        });
                    },
                }
            },
            None => { break; },
        }
    }

    if pointvec.len() > 0 {
        bind_framebuffer(FRAMEBUFFER, framebuffer);
        let safe_matrix: &Matrix = unsafe { mem::transmute(matrix) };
        shader.prep(safe_matrix.as_slice(), pointvec.as_slice(), color, brush, backBuffer);
        draw_arrays(POINTS, 0, pointvec.len() as i32);
        check_gl_error("draw_arrays");
    }

    s.pointCounter = pointCounter;
}

#[allow(non_snake_case_functions)]
extern "C" {
    pub fn doInterpolateLua(startpoint: *const ShaderPaintPoint, endpoint: *const ShaderPaintPoint, x: i32, y: i32, output: *mut Vec<ShaderPaintPoint>, callback: unsafe extern "C" fn(*const ShaderPaintPoint, i32, &mut Vec<ShaderPaintPoint>)->());
}

#[no_mangle]
pub unsafe extern "C" fn pushrustvec(vec: &mut Vec<ShaderPaintPoint>, point: *const ShaderPaintPoint) {
    vec.push(*point);
}
unsafe extern "C" fn interpolate_callback(points: *const ShaderPaintPoint, count: i32, output: &mut Vec<ShaderPaintPoint>) {
    slice::raw::buf_as_slice(points, count as uint, |slice| {
        output.push_all(slice);
    });
}

fn interpolate_lua_from_rust(a: ShaderPaintPoint, b: ShaderPaintPoint, dimensions: (i32, i32), output: &mut Vec<ShaderPaintPoint>) -> () {
    unsafe {
        let (x,y) = dimensions;
        doInterpolateLua(&a, &b, x, y, output, interpolate_callback);
    }
}

