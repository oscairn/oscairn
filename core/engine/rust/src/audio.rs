//! A plan's audio on the engine thread (engine_core.h's oscairn_audio): its buffers, the audio
//! connections into its steps and the graph's outputs, the output guard, meters and the activity a
//! step's sound keeps. Laid out for a memory-bound core: every buffer in one allocation in run order
//! (the external input, then each step's inputs and outputs), every connection in one array in run
//! order with where it reads resolved when the plan is built, the steps in one array of small records.

use crate::bindings::{oscairn_audio_route as Connection, oscairn_audio_step as StepIn};
use crate::budget::Activity;
use crate::{Buffers, Channels, Shared};
use std::ops::Range;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};

/// A node's meter (Graph::meterChannels floats, as atomics).
pub type Peaks = [AtomicU32; 16];
const OUTPUT_BOUND: f32 = 1e6; // Graph::outputBound: +120 dBFS

/// oscairn_bus, as it is kept.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Bus {
    pub offset: u32,
    pub channels: u32,
    pub peaks: Option<Shared<Peaks>>,
    pub activity: Option<Shared<Activity>>,
}

/// A connection, the buffer it reads (`at`), whether it is the first into its channel, and the ramp
/// a played change gives its gain (now, delta, left).
struct Route {
    c: Connection,
    at: u32,
    first: bool,
    now: f32,
    delta: f32,
    left: u32,
}
/// A step: its first input and output buffers, its connections, silent inputs and buses (ranges of
/// Audio's arrays), and the output guard's frames faded since the last good block.
struct Step {
    host: usize,
    input: u32,
    output: u32,
    outputs: u32,
    metered: u32,
    peaks: Option<Shared<Peaks>>,
    activity: Option<Shared<Activity>>,
    routes: Range<u32>,
    silent: Range<u32>,
    buses: Range<u32>,
    faded_for: u32,
}

pub struct Audio {
    inputs: u32,
    channels: u32, // the graph's outputs
    buffers: Buffers,
    pointers: Vec<*mut f32>, // each buffer's, for the plugin hosts
    last: Vec<f32>,          // each buffer's last good sample (the output guard)
    peaks: Vec<f32>,         // each buffer's peak this block, once its step finished
    steps: Vec<Step>,
    routes: Vec<Route>, // each step's, then the outputs'
    silent: Vec<u32>,   // each step's inputs no connection reaches, then the outputs'
    buses: Vec<Bus>,
    outputs: (Range<u32>, Range<u32>), // the outputs' connections and silent channels
}

/// `routes` added to `into`, each marked first into its channel or not; the channels of `channels`
/// none reaches added to `silent`; their ranges.
fn connect(into: &mut Vec<Route>, silent: &mut Vec<u32>, routes: &[Connection], channels: u32, at: impl Fn(&Connection) -> u32) -> (Range<u32>, Range<u32>) {
    let (r, s) = (into.len() as u32, silent.len() as u32);
    let mut written = vec![false; channels as usize];
    for c in routes {
        into.push(Route { c: *c, at: at(c), first: !written[c.to as usize], now: c.gain, delta: 0.0, left: 0 });
        written[c.to as usize] = true;
    }
    silent.extend((0..channels).filter(|&c| !written[c as usize]));
    (r..into.len() as u32, s..silent.len() as u32)
}

fn range<'a, T>(v: &'a [T], r: &Range<u32>) -> &'a [T] {
    &v[r.start as usize..r.end as usize]
}

impl Audio {
    pub fn new(steps: &[StepIn], outputs: &[Connection], inputs: u32, channels: u32, max_frames: u32) -> Audio {
        let mut buffer = inputs;
        let firsts: Vec<(u32, u32)> = steps
            .iter()
            .map(|s| {
                buffer += s.inputs + s.outputs;
                (buffer - s.inputs - s.outputs, buffer - s.outputs)
            })
            .collect();
        let at = |c: &Connection| if c.source < 0 { c.from } else { firsts[c.source as usize].1 + c.from };
        let (mut routes, mut silent, mut buses) = (Vec::new(), Vec::new(), Vec::new());
        let steps = steps
            .iter()
            .zip(&firsts)
            .map(|(s, &(input, output))| {
                let (r, quiet) = connect(&mut routes, &mut silent, s.routes(), s.inputs, at);
                buses.extend_from_slice(s.buses());
                Step {
                    host: s.host as usize,
                    input,
                    output,
                    outputs: s.outputs,
                    metered: s.metered,
                    peaks: NonNull::new(s.peaks).map(Shared),
                    activity: NonNull::new(s.activity).map(Shared),
                    routes: r,
                    silent: quiet,
                    buses: (buses.len() - s.buses().len()) as u32..buses.len() as u32,
                    faded_for: 0,
                }
            })
            .collect();
        let outputs = connect(&mut routes, &mut silent, outputs, channels, at);
        let buffers = Buffers::new(buffer as usize, max_frames as usize);
        Audio {
            inputs,
            channels,
            pointers: (0..buffer as usize).map(|b| buffers.pointer(b)).collect(),
            buffers,
            last: vec![0.0; buffer as usize],
            peaks: vec![0.0; buffer as usize],
            steps,
            routes,
            silent,
            buses,
            outputs,
        }
    }

    pub fn output_channels(&self) -> u32 {
        self.channels
    }

    /// A step's input or output buffers' pointers, or with None the external input's.
    pub fn pointers(&self, step: Option<usize>, outputs: bool) -> &[*mut f32] {
        match step {
            None => &self.pointers[..self.inputs as usize],
            Some(s) => {
                let s = &self.steps[s];
                let first = if outputs { s.output } else { s.input } as usize;
                &self.pointers[first..if outputs { first + s.outputs as usize } else { s.output as usize }]
            }
        }
    }

    /// A played change: each connection in both plans ramps from the gain it has in `was` to its own.
    pub fn carry(&mut self, was: &Audio, ramp: u32) {
        let host = |a: &Audio, s: i32| if s < 0 { 0 } else { a.steps[s as usize].host };
        let carry = |now: Range<u32>, then: Range<u32>, steps: &[Step], routes: &mut [Route]| {
            for r in &mut routes[now.start as usize..now.end as usize] {
                let (rs, source) = (r.c.source, if r.c.source < 0 { 0 } else { steps[r.c.source as usize].host });
                if let Some(o) = range(&was.routes, &then)
                    .iter()
                    .find(|o| o.c.from == r.c.from && o.c.to == r.c.to && host(was, o.c.source) == source && (o.c.source < 0) == (rs < 0))
                {
                    r.now = if o.left != 0 { o.now } else { o.c.gain };
                    if r.now != r.c.gain {
                        r.left = ramp;
                        r.delta = (r.c.gain - r.now) / ramp as f32;
                    }
                }
            }
        };
        carry(self.outputs.0.clone(), was.outputs.0.clone(), &self.steps, &mut self.routes);
        for s in &self.steps {
            if let Some(old) = was.steps.iter().find(|o| o.host == s.host) {
                carry(s.routes.clone(), old.routes.clone(), &self.steps, &mut self.routes);
            }
        }
    }

    /// A block starts: the external input copied in, the outputs no connection reaches and those past
    /// the graph's own silenced.
    pub fn begin(&mut self, inputs: &Channels, outputs: &mut Channels, frames: usize) {
        for c in 0..self.inputs {
            let dest = self.buffers.get_mut(c as usize, frames);
            match inputs.get(c as usize, frames) {
                Some(x) => dest.copy_from_slice(x),
                None => dest.fill(0.0),
            }
        }
        for &c in range(&self.silent, &self.outputs.1) {
            if let Some(o) = outputs.get_mut(c as usize, frames) {
                o.fill(0.0);
            }
        }
        for c in self.channels as usize..outputs.count() {
            if let Some(o) = outputs.get_mut(c, frames) {
                o.fill(0.0);
            }
        }
    }

    /// A step's inputs mixed from its connections, or with `step` past the last the graph's outputs.
    pub fn mix(&mut self, step: usize, outputs: &mut Channels, frames: usize, live_from: &[f32], live_to: &[f32]) {
        let Some(s) = self.steps.get(step) else {
            for r in &mut self.routes[self.outputs.0.start as usize..self.outputs.0.end as usize] {
                if let Some(dest) = outputs.get_mut(r.c.to as usize, frames) {
                    mix(r, self.buffers.get(r.at as usize, frames), dest, live_from, live_to);
                }
            }
            return;
        };
        for &c in range(&self.silent, &s.silent) {
            self.buffers.get_mut((s.input + c) as usize, frames).fill(0.0);
        }
        for r in &mut self.routes[s.routes.start as usize..s.routes.end as usize] {
            let (from, dest) = self.buffers.pair(r.at as usize, (s.input + r.c.to) as usize, frames);
            mix(r, from, dest, live_from, live_to);
        }
    }

    /// After a step's plugin rendered: the output guard, its meters, its and its buses' activity.
    /// Every input sample of a step zero.
    pub fn quiet(&self, step: usize, frames: usize) -> bool {
        let s = &self.steps[step];
        // One pass without an exit (it vectorizes; an early exit would run the whole block for silence, the common case): sign bits dropped, so -0.0 is zero.
        (s.input..s.output).all(|b| self.buffers.get(b as usize, frames).iter().fold(0u32, |m, x| m | x.to_bits() << 1) == 0)
    }
    /// A step's outputs zeroed: a block its plugin slept through.
    pub fn silence(&mut self, step: usize, frames: usize) {
        let s = &self.steps[step];
        for b in s.output..s.output + s.outputs {
            self.buffers.get_mut(b as usize, frames).fill(0.0);
        }
    }

    pub fn finish(&mut self, step: usize, frames: usize, processed: bool, asleep: bool, fade: u32) {
        let s = &mut self.steps[step];
        let outs = s.output as usize..(s.output + s.outputs) as usize;
        // Each channel's peak, read once for the guard, the meters and the quiet: one pass over the block.
        if processed {
            let mut most = 0;
            for b in outs.clone() {
                let m = magnitude(self.buffers.get(b, frames));
                self.peaks[b] = f32::from_bits(m); // a bad block's, measured again once faded
                most = most.max(m);
            }
            let bad = most > OUTPUT_BOUND.to_bits();
            let count = |a: &Activity| {
                if bad {
                    a.bad_output.fetch_add(1, Relaxed);
                    a.bad_run.store(a.bad_run.load(Relaxed) + 1, Relaxed);
                } else {
                    a.bad_run.store(0, Relaxed);
                }
            };
            match s.activity {
                Some(a) => count(&a),
                None => range(&self.buses, &s.buses).iter().filter_map(|b| b.activity).for_each(|a| count(&a)),
            }
            if !bad {
                for b in outs.clone() {
                    self.last[b] = self.buffers.get(b, frames)[frames - 1];
                }
                s.faded_for = 0;
            } else {
                for b in outs.clone() {
                    let last = self.last[b];
                    for (f, x) in self.buffers.get_mut(b, frames).iter_mut().enumerate() {
                        let f = f as u32;
                        *x = if s.faded_for + f < fade { last * (fade - 1 - s.faded_for - f) as f32 / fade as f32 } else { 0.0 };
                    }
                    self.peaks[b] = f32::from_bits(magnitude(self.buffers.get(b, frames)));
                }
                s.faded_for = fade.min(s.faded_for + frames as u32);
            }
        }
        let loudest = |b: Range<usize>| self.peaks[b].iter().fold(0f32, |p, &x| p.max(x));
        let meter = |peaks: Option<Shared<Peaks>>, first: usize, count: u32| {
            let Some(peaks) = peaks else { return };
            for (c, held) in peaks.iter().enumerate().take(count as usize) {
                let peak = loudest(first + c..first + c + 1);
                let mut seen = held.load(Relaxed);
                while peak > f32::from_bits(seen) {
                    match held.compare_exchange_weak(seen, peak.to_bits(), Relaxed, Relaxed) {
                        Ok(_) => break,
                        Err(now) => seen = now,
                    }
                }
            }
        };
        let quiet = |a: Option<Shared<Activity>>, b: Range<usize>| {
            let Some(a) = a else { return };
            if processed {
                a.quiet.store(if loudest(b) < 1e-5 { a.quiet.load(Relaxed) + frames as u64 } else { 0 }, Relaxed);
                a.asleep.store(asleep, Relaxed);
            }
            a.age.store(a.age.load(Relaxed) + frames as u64, Relaxed);
        };
        if processed {
            meter(s.peaks, s.output as usize, s.metered);
        }
        for b in range(&self.buses, &s.buses) {
            let first = (s.output + b.offset) as usize;
            if processed {
                meter(b.peaks, first, b.channels);
            }
            quiet(b.activity, first..first + b.channels as usize);
        }
        quiet(s.activity, outs);
    }
}

/// A channel's largest magnitude, as its bits: above OUTPUT_BOUND's for a sample past it, infinite or
/// NaN (their bits are larger); otherwise its peak, as positive floats order as their bits do. An
/// integer max, so it vectorizes.
fn magnitude(x: &[f32]) -> u32 {
    x.iter().fold(0, |m, x| m.max(x.to_bits() & 0x7fff_ffff))
}

/// `from` into `dest` through `r`: scaled by its live gains, ramped across the block from the last
/// block's product, and by its gain, ramped from the last plan's; the first into its channel sets it.
fn mix(r: &mut Route, from: &[f32], dest: &mut [f32], live_from: &[f32], live_to: &[f32]) {
    let frames = dest.len();
    let (first, gain) = (r.first, r.c.gain);
    let ramp = |r: &mut Route| {
        if r.left != 0 {
            r.now += r.delta;
            r.left -= 1;
            if r.left == 0 {
                r.now = gain;
            }
        }
        r.now
    };
    if r.c.live[0] != 0 {
        let (mut start, mut end) = (1f32, 1f32);
        for slot in r.c.live.iter().filter(|&&s| s != 0) {
            start *= live_from[*slot as usize];
            end *= live_to[*slot as usize];
        }
        if start != end || r.left != 0 {
            let step = (end - start) / frames as f32;
            for i in 0..frames {
                let g = ramp(r) * (start + step * (i + 1) as f32);
                dest[i] = if first { from[i] * g } else { dest[i] + from[i] * g };
            }
            return;
        }
        return scale(from, dest, first, gain * end);
    }
    if r.left != 0 {
        for i in 0..frames {
            let g = ramp(r);
            dest[i] = if first { from[i] * g } else { dest[i] + from[i] * g };
        }
        return;
    }
    scale(from, dest, first, gain);
}

fn scale(from: &[f32], dest: &mut [f32], first: bool, g: f32) {
    match (first, g == 1.0) {
        (true, true) => dest.copy_from_slice(from),
        (true, false) => dest.iter_mut().zip(from).for_each(|(d, x)| *d = x * g),
        (false, true) => dest.iter_mut().zip(from).for_each(|(d, x)| *d += x),
        (false, false) => dest.iter_mut().zip(from).for_each(|(d, x)| *d += x * g),
    }
}
