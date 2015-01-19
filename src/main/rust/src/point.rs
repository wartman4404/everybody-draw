// TODO: more meaningful names
use std::sync::spsc_queue;

#[deriving(Clone, Show, PartialEq)]
#[repr(C)]
pub struct Coordinate {
    pub x: f32,
    pub y: f32,
}

/// Holds data from motionevent entries.
#[deriving(Clone, Show, PartialEq)]
#[repr(C)]
pub struct PaintPoint {
    pub pos: Coordinate,
    pub time: f32, // floating-point seconds
    pub size: f32,
}

/// Holds raw data used for pointshader attribs.
/// These fields overlap with PaintPoint somewhat but aren't necessarily directly sourced from one
/// so adding it as a child doesn't seem ideal
#[deriving(Clone, Show)]
#[repr(C)]
pub struct ShaderPaintPoint {
    pub pos: Coordinate,
    pub time: f32,
    pub size: f32,
    pub speed: f32,
    pub distance: f32,
    pub counter: f32, // could become a uniform? only floating-point allowed for attribs
}

/// Pointer state, corresponding to a single motionevent historical entry
/// Stop, unsurprisingly, indicates a pointer has been lifted
/// this enables us to use raw motionevent pointer ids, which get recycled regularly
/// it's arguably simpler than ensuring each pointer gets a unique queue for its entire
/// lifetime and maintaining an up-to-date pointer id -> queue mapping
#[deriving(PartialEq)]
pub enum GenericPointInfo<T> {
    Stop,
    Point(T),
}
pub type PointInfo = GenericPointInfo<PaintPoint>;

/// Preprocessed queues for multiple interpolators have the same need.  This design choice
/// is probably worth reevaluating.
pub type ShaderPointInfo = GenericPointInfo<ShaderPaintPoint>;

/// A single entry in the point queue.
#[deriving(PartialEq)]
pub struct PointEntry {
    pub index: i32,
    pub entry: PointInfo,
}

pub type PointConsumer = spsc_queue::Consumer<PointEntry>;
pub type PointProducer = spsc_queue::Producer<PointEntry>;