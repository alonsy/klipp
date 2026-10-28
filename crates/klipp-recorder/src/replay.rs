//! The replay buffer: the last N seconds of encoded packets, held in RAM.
//!
//! Packet bytes go into fixed-size chunks that are recycled. After warm-up,
//! pushing a packet doesn't allocate, and memory follows the amount of data
//! actually held rather than the worst-case bitrate.

use std::collections::{BTreeMap, VecDeque};

const CHUNK_SIZE: usize = 1 << 20;
/// Spare chunks kept for reuse. Extra ones are freed, so memory shrinks again
/// after a high-bitrate stretch.
const MAX_SPARE_CHUNKS: usize = 4;

/// Stream index of the video track. Audio tracks are 1 and up.
pub const VIDEO: u8 = 0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PacketInfo {
    pub stream: u8,
    /// Capture time on the shared clock, in 100 ns units.
    pub time: i64,
    /// Timestamps in the stream's own time base.
    pub pts: i64,
    pub dts: i64,
    pub duration: i64,
    pub key: bool,
}

#[derive(Clone, Copy)]
struct Entry {
    info: PacketInfo,
    seq: u64,
    /// Position in the logical byte stream (see `ReplayBuffer::write_pos`).
    offset: u64,
    len: usize,
}

pub struct ReplayBuffer {
    /// How much history to keep, in 100 ns units.
    window: i64,
    /// Hard cap on held bytes. Can be exceeded by at most one GOP.
    max_bytes: u64,
    /// Chunk `i` holds logical bytes `[(first_chunk + i) * CHUNK_SIZE, ...)`.
    chunks: VecDeque<Box<[u8]>>,
    first_chunk: u64,
    /// Logical offset of the next byte to write. Only ever grows.
    write_pos: u64,
    spare: Vec<Box<[u8]>>,
    entries: VecDeque<Entry>,
    /// Video keyframes still in the buffer: (seq, time). The front entry is always the oldest.
    keyframes: VecDeque<(u64, i64)>,
    next_seq: u64,
    newest_time: i64,
}

/// Packets copied out of the buffer for saving.
pub struct Clip {
    /// Capture time of the first video frame.
    pub start_time: i64,
    pub packets: Vec<(PacketInfo, std::ops::Range<usize>)>,
    pub data: Vec<u8>,
}

impl ReplayBuffer {
    pub fn new(window_100ns: i64, max_bytes: u64) -> Self {
        Self {
            window: window_100ns,
            max_bytes,
            chunks: VecDeque::new(),
            first_chunk: 0,
            write_pos: 0,
            spare: Vec::new(),
            entries: VecDeque::new(),
            keyframes: VecDeque::new(),
            next_seq: 0,
            newest_time: i64::MIN,
        }
    }

    pub fn push(&mut self, info: PacketInfo, data: &[u8]) {
        let seq = self.next_seq;
        self.next_seq += 1;
        let offset = self.write_pos;
        self.write(data);
        self.entries.push_back(Entry { info, seq, offset, len: data.len() });
        if info.stream == VIDEO && info.key {
            self.keyframes.push_back((seq, info.time));
        }
        self.newest_time = self.newest_time.max(info.time);
        self.evict();
    }

    /// Copies out a clip of at least `length` (100 ns units) ending at `end`
    /// (clamped to the newest packet), starting at a video keyframe. Ending at
    /// a fixed time lets every track stop together even though audio arrives
    /// slightly after video. Returns `None` if there is no keyframe yet.
    pub fn snapshot(&self, end: i64, length: i64) -> Option<Clip> {
        let end = end.min(self.newest_time);
        let target = end.saturating_sub(length);
        let &(kf_seq, kf_time) = self.keyframes.iter().rev().find(|&&(_, t)| t <= target).or(self.keyframes.front())?;
        let front_seq = self.entries.front()?.seq;
        let kf_offset = self.entries[(kf_seq - front_seq) as usize].offset;

        let mut data = Vec::with_capacity((self.write_pos - kf_offset) as usize);
        let mut packets = Vec::new();
        for e in &self.entries {
            // Audio is selected by time: it can arrive before the video frame it plays with.
            let keep = e.info.time <= end && if e.info.stream == VIDEO { e.seq >= kf_seq } else { e.info.time >= kf_time };
            if keep {
                let start = data.len();
                self.read_into(e.offset, e.len, &mut data);
                packets.push((e.info, start..data.len()));
            }
        }
        Some(Clip { start_time: kf_time, packets, data })
    }

    /// Appends to `clip` what each stream recorded after it, up to `end`, so
    /// hotkey presses close together make one continuous clip. Returns false
    /// (and leaves the clip alone) if the buffer no longer reaches back to
    /// where the clip ends.
    pub fn extend(&self, clip: &mut Clip, end: i64) -> bool {
        let end = end.min(self.newest_time);
        let mut last: BTreeMap<u8, i64> = BTreeMap::new();
        for (p, _) in &clip.packets {
            let t = last.entry(p.stream).or_insert(p.time);
            *t = (*t).max(p.time);
        }
        let (Some(front), Some(&video_end)) = (self.entries.front(), last.get(&VIDEO)) else { return false };
        if front.info.time > video_end {
            return false;
        }
        for e in &self.entries {
            let after = last.get(&e.info.stream).copied().unwrap_or(clip.start_time - 1);
            if e.info.time > after && e.info.time <= end {
                let start = clip.data.len();
                self.read_into(e.offset, e.len, &mut clip.data);
                clip.packets.push((e.info, start..clip.data.len()));
            }
        }
        true
    }

    /// Changes how much history is kept. Shrinking evicts right away; growing
    /// just stops evicting until the buffer has filled up to the new window.
    pub fn set_limits(&mut self, window_100ns: i64, max_bytes: u64) {
        self.window = window_100ns;
        self.max_bytes = max_bytes;
        self.evict();
    }

    /// Drops everything, e.g. when the encoder restarts with new settings.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.keyframes.clear();
        self.newest_time = i64::MIN;
        self.evict();
    }

    /// Bytes of packet data currently held.
    pub fn held_bytes(&self) -> u64 {
        self.entries.front().map_or(0, |e| self.write_pos - e.offset)
    }

    /// Every byte of packet data pushed so far (held or not): the bitrate, over time.
    pub fn written_bytes(&self) -> u64 {
        self.write_pos
    }

    /// Memory allocated for packet data, including spare chunks.
    pub fn allocated_bytes(&self) -> u64 {
        ((self.chunks.len() + self.spare.len()) * CHUNK_SIZE) as u64
    }

    /// Time span from the oldest keyframe to the newest packet, in 100 ns units.
    pub fn duration(&self) -> i64 {
        self.keyframes.front().map_or(0, |&(_, t)| self.newest_time - t)
    }

    fn write(&mut self, mut data: &[u8]) {
        while !data.is_empty() {
            let chunk = self.write_pos / CHUNK_SIZE as u64;
            if self.chunks.is_empty() {
                self.first_chunk = chunk;
            }
            if chunk >= self.first_chunk + self.chunks.len() as u64 {
                let fresh = self.spare.pop().unwrap_or_else(|| vec![0u8; CHUNK_SIZE].into_boxed_slice());
                self.chunks.push_back(fresh);
            }
            let within = (self.write_pos % CHUNK_SIZE as u64) as usize;
            let n = data.len().min(CHUNK_SIZE - within);
            let dest = &mut self.chunks[(chunk - self.first_chunk) as usize];
            dest[within..within + n].copy_from_slice(&data[..n]);
            self.write_pos += n as u64;
            data = &data[n..];
        }
    }

    fn read_into(&self, mut offset: u64, mut len: usize, out: &mut Vec<u8>) {
        while len > 0 {
            let chunk = (offset / CHUNK_SIZE as u64 - self.first_chunk) as usize;
            let within = (offset % CHUNK_SIZE as u64) as usize;
            let n = len.min(CHUNK_SIZE - within);
            out.extend_from_slice(&self.chunks[chunk][within..within + n]);
            offset += n as u64;
            len -= n;
        }
    }

    /// Drops whole GOPs from the front while the rest still covers the window,
    /// or while over the byte cap, then recycles chunks nothing points into.
    fn evict(&mut self) {
        while self.keyframes.len() >= 2 {
            let (next_seq, next_time) = self.keyframes[1];
            let covers_window = self.newest_time - next_time >= self.window;
            if !covers_window && self.held_bytes() <= self.max_bytes {
                break;
            }
            while self.entries.front().is_some_and(|e| e.seq < next_seq) {
                self.entries.pop_front();
            }
            self.keyframes.pop_front();
        }

        let keep_from = self.entries.front().map_or(self.write_pos, |e| e.offset) / CHUNK_SIZE as u64;
        while self.first_chunk < keep_from {
            let Some(chunk) = self.chunks.pop_front() else { break };
            self.first_chunk += 1;
            if self.spare.len() < MAX_SPARE_CHUNKS {
                self.spare.push(chunk);
            }
        }
    }
}
