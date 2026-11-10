//! The desktop's speakers: the decisions are the same as the browser's (`super`), the desk is
//! `el_client_core::fx::out`'s renderer on the sound card's own thread (cpal), told what to do
//! through a ring that never allocates (rtrb), as Gates' `audio_out.rs` feeds its own (Bevy's
//! audio re-made a resampler inside the callback, allocating, so neither game uses it). The files
//! are the MP3s beside the Opus ones (the browser's choice where it can't play Opus), decoded by
//! symphonia on a thread of their own.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;

use bevy::prelude::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use el_client_core::fx::out::{BLOCK, Cmd, Pcm, Renderer, Route};

/// Voices sounding at once, and sounds held.
const VOICES: usize = 64;
const SLOTS: usize = 1_024;
/// Commands a frame may send before the desk takes them.
const RING: usize = 4_096;

/// A sound playing, by its voice.
pub type Voice = u32;

/// What the sound card's thread owns.
struct Feed {
    desk: Renderer,
    cmds: rtrb::Consumer<Cmd>,
    /// One block rendered and not yet all handed over.
    carry: [f32; 2 * BLOCK],
    used: usize,
    clock: Arc<AtomicU64>,
}

impl Feed {
    fn fill<T: cpal::SizedSample + cpal::FromSample<f32>>(&mut self, out: &mut [T], channels: usize) {
        while let Ok(c) = self.cmds.pop() {
            self.desk.apply(c);
        }
        for frame in out.chunks_mut(channels.max(1)) {
            if self.used >= 2 * BLOCK {
                self.desk.render(&mut self.carry);
                self.used = 0;
            }
            let (l, r) = (self.carry[self.used], self.carry[self.used + 1]);
            self.used += 2;
            match frame.len() {
                1 => frame[0] = T::from_sample((l + r) * 0.5),
                _ => {
                    frame[0] = T::from_sample(l);
                    frame[1] = T::from_sample(r);
                    for x in frame.iter_mut().skip(2) {
                        *x = T::from_sample(0.0);
                    }
                }
            }
        }
        self.clock.store(self.desk.frames(), Ordering::Relaxed);
    }
}

/// The desktop's output: the game thread's half.
pub struct Out {
    cmds: Option<rtrb::Producer<Cmd>>,
    _stream: Option<cpal::Stream>,
    rate: u32,
    clock: Arc<AtomicU64>,
    root: PathBuf,
    /// The MP3s there are, by stem.
    listed: HashSet<String>,
    asked: HashSet<String>,
    /// Decoded, each with its slot on the desk.
    held: HashMap<String, (u16, Arc<Pcm>)>,
    decoded: mpsc::Receiver<(String, Option<Pcm>)>,
    decode: mpsc::Sender<(String, Option<Pcm>)>,
    next_voice: u32,
    failed: u32,
    /// Commands dropped for a full ring, or for no device.
    pub dropped: u32,
    state: &'static str,
}

fn open(rate_out: &mut u32, clock: Arc<AtomicU64>) -> Result<(cpal::Stream, rtrb::Producer<Cmd>), String> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("no sound device")?;
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    let channels = usize::from(config.channels());
    *rate_out = config.sample_rate().0;
    let (tx, rx) = rtrb::RingBuffer::<Cmd>::new(RING);
    let mut feed = Feed {
        desk: Renderer::new(*rate_out, VOICES, SLOTS),
        cmds: rx,
        carry: [0.0; 2 * BLOCK],
        used: 2 * BLOCK,
        clock,
    };
    let err = |e: cpal::StreamError| warn!("sound: {e}");
    let cfg = config.config();
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => {
            device.build_output_stream(&cfg, move |o: &mut [f32], _: &_| feed.fill(o, channels), err, None)
        }
        cpal::SampleFormat::I16 => {
            device.build_output_stream(&cfg, move |o: &mut [i16], _: &_| feed.fill(o, channels), err, None)
        }
        cpal::SampleFormat::U16 => {
            device.build_output_stream(&cfg, move |o: &mut [u16], _: &_| feed.fill(o, channels), err, None)
        }
        cpal::SampleFormat::I32 => {
            device.build_output_stream(&cfg, move |o: &mut [i32], _: &_| feed.fill(o, channels), err, None)
        }
        other => return Err(format!("the device wants {other:?} samples")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, tx))
}

/// Decodes an MP3 into samples (all of it: the effects are short, the music a few MB).
fn decode(path: &std::path::Path) -> Option<Pcm> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;
    let file = std::fs::File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("mp3");
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions { enable_gapless: true, ..Default::default() },
            &MetadataOptions::default(),
        )
        .ok()?;
    let mut format = probed.format;
    let track = format.default_track()?;
    let id = track.id;
    let mut dec =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default()).ok()?;
    let (mut rate, mut channels, mut frames) = (0u32, 0u8, Vec::new());
    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != id {
            continue;
        }
        let Ok(buf) = dec.decode(&packet) else { continue };
        let spec = *buf.spec();
        rate = spec.rate;
        channels = spec.channels.count().min(2) as u8;
        let mut sb = SampleBuffer::<f32>::new(buf.capacity() as u64, spec);
        sb.copy_interleaved_ref(buf);
        let all = spec.channels.count();
        if all <= 2 {
            frames.extend_from_slice(sb.samples());
        } else {
            for f in sb.samples().chunks(all) {
                frames.extend_from_slice(&f[..2]);
            }
        }
    }
    (rate > 0 && !frames.is_empty()).then(|| Pcm { rate, channels, frames: frames.into_boxed_slice() })
}

impl Out {
    /// The sound card, or none (a machine with no device still plays, silently).
    pub fn new(asset_root: &std::path::Path) -> Out {
        let clock = Arc::new(AtomicU64::new(0));
        let mut rate = 48_000;
        let (stream, cmds, state) = match open(&mut rate, clock.clone()) {
            Ok((s, tx)) => (Some(s), Some(tx), "running"),
            Err(e) => {
                info!("no sound: {e}");
                (None, None, "none")
            }
        };
        let root = asset_root.join("audio");
        let listed = std::fs::read_dir(&root)
            .map(|d| {
                d.filter_map(|e| e.ok())
                    .filter_map(|e| e.file_name().to_str()?.strip_suffix(".mp3").map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        let (decode, decoded) = mpsc::channel();
        Out {
            cmds,
            _stream: stream,
            rate,
            clock,
            root,
            listed,
            asked: HashSet::new(),
            held: HashMap::new(),
            decoded,
            decode,
            next_voice: 1,
            failed: 0,
            dropped: 0,
            state,
        }
    }

    fn send(&mut self, c: Cmd) -> bool {
        let sent = self.cmds.as_mut().is_some_and(|tx| tx.push(c).is_ok());
        if !sent {
            self.dropped += 1;
        }
        sent
    }

    /// The desk's clock, s (the frame's clock without a device).
    pub fn time(&self) -> f64 {
        match self.cmds {
            Some(_) => self.clock.load(Ordering::Relaxed) as f64 / f64::from(self.rate),
            None => crate::platform::now_s(),
        }
    }

    pub fn state(&self) -> &'static str {
        self.state
    }

    pub fn format(&self) -> &'static str {
        "mp3"
    }

    pub fn listed(&self) -> bool {
        true
    }

    pub fn loaded(&self) -> usize {
        self.held.len()
    }

    /// Decodes `stem` on a thread, once (if there's a file for it).
    pub fn want(&mut self, stem: &str) {
        if !self.listed.contains(stem) || !self.asked.insert(stem.to_string()) {
            return;
        }
        let (path, stem, tx) = (self.root.join(format!("{stem}.mp3")), stem.to_string(), self.decode.clone());
        let _ = std::thread::Builder::new().name("el-decode".into()).spawn(move || {
            let _ = tx.send((stem, decode(&path)));
        });
    }

    /// The decoded sounds onto the desk.
    pub fn poll(&mut self) {
        while let Ok((stem, pcm)) = self.decoded.try_recv() {
            match pcm {
                Some(pcm) if self.held.len() < SLOTS => {
                    let slot = self.held.len() as u16;
                    let pcm = Arc::new(pcm);
                    self.held.insert(stem, (slot, pcm.clone()));
                    self.send(Cmd::Install { slot, pcm });
                }
                _ => self.failed += 1,
            }
        }
    }

    pub fn duration(&self, stem: &str) -> Option<f64> {
        self.held.get(stem).map(|(_, p)| p.secs())
    }

    /// Starts `stem` on `route`; the voice and its length, if it started.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &mut self,
        stem: &str,
        route: Route,
        gain: f32,
        pan: Option<f32>,
        lowpass: f32,
        cents: f32,
        looped: bool,
    ) -> Option<(Voice, f64)> {
        let (slot, secs) = self.held.get(stem).map(|(s, p)| (*s, p.secs()))?;
        let voice = self.next_voice;
        self.next_voice = self.next_voice.wrapping_add(1).max(1);
        let rate = 2f32.powf(cents / 1_200.0);
        self.send(Cmd::Start { voice, slot, route, gain, pan, lowpass, rate, looped })
            .then_some((voice, secs / f64::from(rate)))
    }

    pub fn gain(&mut self, v: Voice, target: f32, tau: f64) {
        self.send(Cmd::Gain { voice: v, target, tau: tau as f32 });
    }

    pub fn pan(&mut self, v: Voice, target: f32, tau: f64) {
        self.send(Cmd::Pan { voice: v, target, tau: tau as f32 });
    }

    /// Faded out (time constant `tau`), and let go once silent.
    pub fn stop(&mut self, v: Voice, tau: f64, _after: f64) {
        self.send(Cmd::Stop { voice: v, fade: tau as f32 });
    }

    pub fn bus(&mut self, k: usize, target: f32, tau: f64) {
        self.send(Cmd::Bus { k: k as u8, target, tau: tau as f32 });
    }

    pub fn master(&mut self, target: f32, tau: f64) {
        self.send(Cmd::Master { target, tau: tau as f32 });
    }

    pub fn back(&mut self, target: f32, tau: f64) {
        self.send(Cmd::Back { target, tau: tau as f32 });
    }

    pub fn failed(&self) -> u32 {
        self.failed
    }

    /// The desk lets a finished voice go by itself.
    pub fn forget(&mut self, _v: Voice) {}

    /// A desktop needs no click before it sounds.
    pub fn unlocked(&self) -> bool {
        true
    }
}
