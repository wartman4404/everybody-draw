use core::prelude::*;
use core::mem;
use collections::vec::Vec;
use collections::{SmallIntMap, MutableMap, MutableSeq, Mutable, Map};
use alloc::boxed::Box;

use std::sync::spsc_queue;

use log::logi;
use motionevent;
use motionevent::append_motion_event;
use android::input::AInputEvent;

use point;
use point::{ShaderPaintPoint, Coordinate, PointEntry, PointConsumer, PointProducer};
use activestate;
use drawevent::Events;
use luascript::LuaScript;
use lua_geom::do_interpolate_lua;


rolling_average_count!(RollingAverage16, 16)

/// lifetime storage for a pointer's past state
struct PointStorage {
    info: Option<ShaderPaintPoint>,
    sizeavg: RollingAverage16<f32>,
    speedavg: RollingAverage16<f32>,
}

#[allow(ctypes)]
pub struct MotionEventConsumer {
    consumer: PointConsumer,
    current_points: SmallIntMap<PointStorage>,
    point_counter: i32,
    point_count: i32,
    all_pointer_state: activestate::ActiveState,
}

pub struct MotionEventProducer {
    pointer_data: motionevent::Data,
    producer: PointProducer,
}

pub struct LuaCallbackType<'a, 'b, 'c, 'd: 'b> {
    consumer: &'a mut MotionEventConsumer,
    events: &'b mut Events<'d>,
    drawvecs: &'c mut [Vec<ShaderPaintPoint>],
}

pub fn create_motion_event_handler() -> (Box<MotionEventConsumer>, Box<MotionEventProducer>) {
    let (consumer, producer) = spsc_queue::queue::<PointEntry>(0);
    let handler = box MotionEventConsumer {
        consumer: consumer,
        current_points: SmallIntMap::new(),
        point_counter: 0, // unique value for each new pointer
        point_count: 0, // # of currently active pointers
        all_pointer_state: activestate::INACTIVE,
    };
    let producer = box MotionEventProducer {
        producer: producer,
        pointer_data: motionevent::Data::new(),
    };
    logi("created statics");
    (handler, producer)
}

pub unsafe fn destroy_motion_event_handler(consumer: Box<MotionEventConsumer>, producer: Box<MotionEventProducer>) {
    mem::drop(consumer);
    mem::drop(producer);
}

//FIXME: needs meaningful name
pub fn jni_append_motion_event(s: &mut MotionEventProducer, evt: *const AInputEvent) {
    append_motion_event(&mut s.pointer_data, evt, &mut s.producer);
}

fn manhattan_distance(a: Coordinate, b: Coordinate) -> f32 {
    let x = if a.x > b.x { a.x - b.x } else { b.x - a.x };
    let y = if a.y > b.y { a.y - b.y } else { b.y - a.y };
    return if x > y { x } else { y };
}

pub fn run_interpolators(dimensions: (i32, i32), s: &mut MotionEventConsumer, events: & mut Events, interpolator: Option<&LuaScript>, drawvecs: & mut [Vec<ShaderPaintPoint>]) -> bool {
    match interpolator {
        Some(interpolator) => {
            interpolator.prep();
            run_lua_shader(dimensions, LuaCallbackType {
                consumer: s,
                events: events,
                drawvecs: drawvecs,
            });
        },
        None => { },
    };
    s.all_pointer_state = s.all_pointer_state.push(s.point_count > 0);
    s.all_pointer_state == activestate::STOPPING
}

#[no_mangle]
pub extern "C" fn next_point_from_lua(data: &mut LuaCallbackType, points: &mut (ShaderPaintPoint, ShaderPaintPoint)) -> bool {
    match next_point(data.consumer, data.events) {
        Some(newpoints) => {
            *points = newpoints;
            true
        },
        None => {
            false
        },
    }
}

fn next_point(s: &mut MotionEventConsumer, e: &mut Events) -> Option<(ShaderPaintPoint, ShaderPaintPoint)> {
    let ref mut queue = s.consumer;
    let ref mut current_points = s.current_points;
    loop {
        match queue.pop() {
            Some(point) => {
                e.pushpoint(point);
                let idx = point.index;
                let newpoint = point.entry;
                if !current_points.contains_key(&(idx as uint)) {
                    current_points.insert(idx as uint, PointStorage {
                        info: None,
                        sizeavg: RollingAverage16::new(),
                        speedavg: RollingAverage16::new(),
                    });
                }
                let oldpoint = current_points.find_mut(&(idx as uint)).unwrap();
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
                        oldpoint.info = Some(npdata);
                        return Some((op, npdata));
                    },
                    (_, point::Stop) => {
                        oldpoint.info = None;
                        oldpoint.sizeavg.clear();
                        oldpoint.speedavg.clear();
                        s.point_count -= 1;
                    },
                    (_, point::Point(p)) => {
                        let old_counter = s.point_counter;
                        s.point_counter += 1;
                        s.point_count += 1;
                        let npdata = ShaderPaintPoint {
                            pos: p.pos,
                            time: p.time,
                            size: p.size,
                            distance: 0f32,
                            speed: 0f32,
                            counter: old_counter as f32,
                        };
                        oldpoint.info = Some(npdata);
                    },
                }
            },
            None => {
                return None;
            }
        }
    }
}

fn run_lua_shader(dimensions: (i32, i32), mut data: LuaCallbackType) {
    let (x,y) = dimensions;
    unsafe {
        do_interpolate_lua(x, y, &mut data as *mut LuaCallbackType as *mut ::libc::c_void);
    }
}

#[no_mangle]
pub unsafe extern "C" fn pushrustvec(data: &mut LuaCallbackType, queue: i32, point: *const ShaderPaintPoint) {
    data.drawvecs[queue as uint].push(*point);
}
