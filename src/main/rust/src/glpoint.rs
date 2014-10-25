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
use drawevent::Events;

rolling_average_count!(RollingAverage16, 16)

/// lifetime storage for a pointer's past state
struct PointStorage {
    info: Option<ShaderPaintPoint>,
    sizeavg: RollingAverage16<f32>,
    speedavg: RollingAverage16<Coordinate>,
}

#[allow(ctypes)]
pub struct MotionEventConsumer {
    consumer: PointConsumer,
    current_points: SmallIntMap<PointStorage>,
    point_counter: i32,
    point_count: i32,
}

pub struct MotionEventProducer {
    pointer_data: motionevent::Data,
    pub producer: PointProducer,
}

pub fn create_motion_event_handler() -> (MotionEventConsumer, MotionEventProducer) {
    let (consumer, producer) = spsc_queue::queue::<PointEntry>(0);
    let handler = MotionEventConsumer {
        consumer: consumer,
        current_points: SmallIntMap::new(),
        point_counter: 0, // unique value for each new pointer
        point_count: 0, // # of currently active pointers
    };
    let producer = MotionEventProducer {
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

#[inline]
pub fn next_point(s: &mut MotionEventConsumer, e: &mut Events) -> (point::ShaderPointEvent, u8) {
    let ref mut queue = s.consumer;
    let ref mut current_points = s.current_points;
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
            let pointevent = match (oldpoint.info, newpoint) {
                (Some(op), point::Point(np)) => {
                    let dist = manhattan_distance(op.pos, np.pos);
                    let avgsize = oldpoint.sizeavg.push(np.size);
                    let avgspeed = oldpoint.speedavg.push(op.pos - np.pos);
                    let npdata = ShaderPaintPoint {
                        pos: np.pos,
                        time: np.time,
                        size: avgsize,
                        speed: avgspeed,
                        distance: op.distance + dist,
                        counter: op.counter,
                    };
                    oldpoint.info = Some(npdata);
                    point::Move(op, npdata)
                },
                (_, point::Stop) => {
                    oldpoint.info = None;
                    oldpoint.sizeavg.clear();
                    oldpoint.speedavg.clear();
                    s.point_count -= 1;
                    point::Up
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
                        speed: Coordinate { x: 0f32, y: 0f32 },
                        counter: old_counter as f32,
                    };
                    oldpoint.info = Some(npdata);
                    point::Down(npdata)
                },
            };
            (pointevent, idx as u8)
        },
        None => {
            (point::NoEvent, 0u8)
        }
    }
}

#[inline]
fn get_count(a: &ShaderPaintPoint, b: &ShaderPaintPoint) -> i32 {
    let distx = if (*a).pos.x > (*b).pos.x { (*a).pos.x - (*b).pos.x } else { (*b).pos.x - (*a).pos.x };
    let disty = if (*a).pos.y > (*b).pos.y { (*a).pos.y - (*b).pos.y } else { (*b).pos.y - (*a).pos.y };
    ((if distx > disty { distx } else { disty }) / 1f32) as i32
}


#[inline]
pub fn push_line(drawvec: &mut Vec<ShaderPaintPoint>, a: &ShaderPaintPoint, b: &ShaderPaintPoint) {
    let count = get_count(a, b);
    let timescale = 10f32;
    let stepx = ((*b).pos.x - (*a).pos.x) / count as f32;
    let stepy = ((*b).pos.y - (*a).pos.y) / count as f32;
    let steptime = ((*b).time - (*a).time) / (count as f32 * timescale);
    let stepsize = ((*b).size - (*a).size) / count as f32;
    let stepspeedx = ((*b).speed.x - (*a).speed.x) / count as f32;
    let stepspeedy = ((*b).speed.y - (*a).speed.y) / count as f32;
    let stepdistance = ((*b).distance - (*a).distance) / count as f32;
    let mut addpoint = *a;
    addpoint.time = (addpoint.time / timescale) % 1f32;
    for _ in range(0, count) {
        drawvec.push(addpoint);
        addpoint.pos.x += stepx;
        addpoint.pos.y += stepy;
        addpoint.time += steptime;
        addpoint.time = if addpoint.time > 1f32 { addpoint.time - 1f32 } else { addpoint.time };
        addpoint.size += stepsize;
        addpoint.speed.x += stepspeedx;
        addpoint.speed.y += stepspeedy;
        addpoint.distance += stepdistance;
    }
}

#[inline]
//pub fn push_catmullrom(drawvec: &mut Vec<ShaderPaintPoint>, a: &ShaderPaintPoint, b: ShaderPaintPoint, c: ShaderPaintPoint, d: ShaderPaintPoint) {
pub fn push_catmullrom(drawvec: &mut Vec<ShaderPaintPoint>, points: &[ShaderPaintPoint, ..4]) {
    push_splinepts::<CatmullRom>(drawvec, points);
}
#[inline]
pub fn push_cubicbezier(drawvec: &mut Vec<ShaderPaintPoint>, points: &[ShaderPaintPoint, ..4]) {
    push_splinepts::<CubicBezier>(drawvec, points);
}
#[inline]
fn push_splinepts<T: Spline<Coordinate>>(drawvec: &mut Vec<ShaderPaintPoint>, points: &[ShaderPaintPoint, ..4]) {
    let coords = unsafe {
        let mut coords: [Coordinate, ..4] = mem::uninitialized();
        for i in range(0, 4) {
            *coords.unsafe_mut(i) = points.unsafe_get(i).pos;
        }
        coords
    };
    let spline: T = Spline::new(coords);
    let (tstart, tend) = spline.get_time_scale();
    let count = unsafe { get_count(points.unsafe_get(1), points.unsafe_get(2)) };
    let timestep = (tend - tstart) / (count as f32);

    let mut addpoint = points[0];
    let mut curtime = tstart;

    let (a, b) = unsafe { (points.unsafe_get(1), points.unsafe_get(2)) };
    let timescale = 10f32;
    let steptime = ((*b).time - (*a).time) / (count as f32 * timescale);
    let stepsize = ((*b).size - (*a).size) / count as f32;
    let stepspeedx = ((*b).speed.x - (*a).speed.x) / count as f32;
    let stepspeedy = ((*b).speed.y - (*a).speed.y) / count as f32;
    let stepdistance = ((*b).distance - (*a).distance) / count as f32;
    addpoint.time = (addpoint.time / timescale) % 1f32;

    for _ in range(0, count) {
        drawvec.push(addpoint);
        addpoint.pos = spline.interpolate(curtime);
        curtime += timestep;

        addpoint.time += steptime;
        addpoint.time = if addpoint.time > 1f32 { addpoint.time - 1f32 } else { addpoint.time };
        addpoint.size += stepsize;
        addpoint.speed.x += stepspeedx;
        addpoint.speed.y += stepspeedy;
        addpoint.distance += stepdistance;
    }
}

trait Spline<T: Mul<f32, T> + Add<T, T> + Sub<T, T>> {
    fn new(points: [T, ..4]) -> Self;
    fn get_time_scale(&self) -> (f32, f32);
    fn interpolate(&self, t: f32) -> T;
}

struct CatmullRom {
    points: [Coordinate, ..4],
    time: [f32, ..4],
}

impl Spline<Coordinate> for CatmullRom {
    fn new(points: [Coordinate, ..4]) -> CatmullRom {
        let mut time = [0f32, ..4];
        let mut total = 0f32;
        unsafe {
            for i in range(0, 3) {
                let (p, pnext) = (points.unsafe_get(i), points.unsafe_get(i+1));
                let Coordinate { x: dx, y: dy } = *pnext - *p;
                total += (dx * dx + dy * dy).powf(0.25f32);
                *time.unsafe_mut(i+1) = total;
            }
        }
        CatmullRom { points: points, time: time }
    }
    fn get_time_scale(&self) -> (f32, f32) {
        (self.time[1], self.time[2])
    }
    fn interpolate(&self, t: f32) -> Coordinate {
        let (p, time) = (self.points, self.time);
        let l01 = p[0] * ((time[1] - t) / (time[1] - time[0])) + p[1] * ((t - time[0]) / (time[1] - time[0]));
        let l12 = p[1] * ((time[2] - t) / (time[2] - time[1])) + p[2] * ((t - time[1]) / (time[2] - time[1]));
        let l23 = p[2] * ((time[3] - t) / (time[3] - time[2])) + p[3] * ((t - time[2]) / (time[3] - time[2]));
        let l012 = l01 * ((time[2] - t) / (time[2] - time[0])) + l12  * ((t - time[0]) / (time[2] - time[0]));
        let l123 = l12 * ((time[3] - t) / (time[3] - time[1])) + l23  * ((t - time[1]) / (time[3] - time[1]));
        let c12 = l012 * ((time[2] - t) / (time[2] - time[1])) + l123 * ((t - time[1]) / (time[2] - time[1]));
        c12
    }
}

struct CubicBezier {
    pub points: [Coordinate, ..4],
}

impl Spline<Coordinate> for CubicBezier {
    fn new(points: [Coordinate, ..4]) -> CubicBezier {
        CubicBezier { points: points }
    }
    fn get_time_scale(&self) -> (f32, f32) {
        (0f32, 1f32)
    }
    fn interpolate(&self, t: f32) -> Coordinate {
        let p = self.points;
        let negt = 1f32 - t;
        let (t2, negt2) = (t * t, negt * negt);
        let p0 = p[0] * (negt * negt2);
        let p1 = p[1] * (negt2 * t * 3f32);
        let p2 = p[2] * (negt * t2 * 3f32);
        let p3 = p[3] * (t * t2);
        p0 + p1 + p2 + p3
    }
}

#[allow(dead_code)]
struct SillyBezier {
    pub bezier: CubicBezier,
}

#[allow(dead_code)]
impl Spline<Coordinate> for SillyBezier {
    fn new(points: [Coordinate, ..4]) -> SillyBezier {
        SillyBezier { bezier: Spline::new(points) }
    }
    fn get_time_scale(&self) -> (f32, f32) {
        let catmullrom: CatmullRom = Spline::new(self.bezier.points);
        catmullrom.get_time_scale()
    }
    fn interpolate(&self, t: f32) -> Coordinate {
        self.bezier.interpolate(t)
    }
}

