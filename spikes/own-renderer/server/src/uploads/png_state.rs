//! Bounded PNG/APNG compressed-stream validation, without pixel conversion.
use super::sniff::be32;
use miniz_oxide::inflate::stream::{InflateState, inflate};
use miniz_oxide::{DataFormat, MZError, MZFlush, MZStatus};

pub(super) const MAX_EXPANDED: usize = 256 * 1024 * 1024;
pub(super) const INTERLEAVED: &str =
    "its animation interleaves ancillary chunks between frame-data pieces";
const INVALID: &str = "its PNG chunks or frame streams are out of order";
const EXTENT: &str = "its decoded scanlines do not have their declared extent";
pub(super) const BUDGET: &str = "its PNG streams exceed the 256 MiB expanded-work limit";

#[derive(Clone, Copy)]
struct Format {
    width: usize,
    height: usize,
    bits: usize,
    interlaced: bool,
    colour: u8,
    depth: u8,
}

impl Format {
    fn new(data: &[u8]) -> Result<Self, &'static str> {
        let width = be32(data, 0).ok_or(INVALID)? as usize;
        let height = be32(data, 4).ok_or(INVALID)? as usize;
        let depth = *data.get(8).ok_or(INVALID)?;
        let colour = *data.get(9).ok_or(INVALID)?;
        let channels = match (colour, depth) {
            (0, 1 | 2 | 4 | 8 | 16) => 1,
            (2, 8 | 16) => 3,
            (3, 1 | 2 | 4 | 8) => 1,
            (4, 8 | 16) => 2,
            (6, 8 | 16) => 4,
            _ => return Err(INVALID),
        };
        if width == 0
            || height == 0
            || width > 0x7fff_ffff
            || height > 0x7fff_ffff
            || data.get(10..12) != Some(&[0, 0])
            || data.get(12).is_none_or(|n| *n > 1)
        {
            return Err(INVALID);
        }
        Ok(Self {
            width,
            height,
            bits: channels * usize::from(depth),
            interlaced: data[12] == 1,
            colour,
            depth,
        })
    }

    fn extent(self, width: usize, height: usize) -> Result<usize, &'static str> {
        let pass = |w: usize, h: usize| -> Option<usize> {
            w.checked_mul(self.bits)?
                .checked_add(7)?
                .checked_div(8)?
                .checked_add(1)?
                .checked_mul(h)
        };
        if !self.interlaced {
            return pass(width, height).ok_or(EXTENT);
        }
        let mut total = 0usize;
        for (x, y, dx, dy) in [
            (0, 0, 8, 8),
            (4, 0, 8, 8),
            (0, 4, 4, 8),
            (2, 0, 4, 4),
            (0, 2, 2, 4),
            (1, 0, 2, 2),
            (0, 1, 1, 2),
        ] {
            if width > x && height > y {
                let size =
                    pass((width - x).div_ceil(dx), (height - y).div_ceil(dy)).ok_or(EXTENT)?;
                total = total.checked_add(size).ok_or(EXTENT)?;
            }
        }
        Ok(total)
    }
}

struct Stream {
    inflater: Box<InflateState>,
    expected: usize,
    expanded: usize,
    ended: bool,
}

impl Stream {
    fn new(expected: usize, remaining: &mut usize) -> Result<Self, &'static str> {
        *remaining = remaining.checked_sub(expected).ok_or(BUDGET)?;
        Ok(Self {
            inflater: InflateState::new_boxed(DataFormat::Zlib),
            expected,
            expanded: 0,
            ended: false,
        })
    }

    fn feed(&mut self, mut input: &[u8]) -> Result<(), &'static str> {
        if self.ended {
            return if input.is_empty() {
                Ok(())
            } else {
                Err("its PNG zlib stream has unused trailing bytes")
            };
        }
        let mut output = [0u8; 32768];
        loop {
            let result = inflate(&mut self.inflater, input, &mut output, MZFlush::None);
            input = input.get(result.bytes_consumed..).ok_or(EXTENT)?;
            self.expanded = self
                .expanded
                .checked_add(result.bytes_written)
                .ok_or(EXTENT)?;
            if self.expanded > self.expected {
                return Err(EXTENT);
            }
            match result.status {
                Ok(MZStatus::StreamEnd) => {
                    self.ended = true;
                    return if input.is_empty() {
                        Ok(())
                    } else {
                        Err("its PNG zlib stream has unused trailing bytes")
                    };
                }
                Ok(MZStatus::Ok) => {}
                Err(MZError::Buf)
                    if input.is_empty()
                        && result.bytes_consumed == 0
                        && result.bytes_written == 0 =>
                {
                    return Ok(());
                }
                _ => return Err("its PNG zlib stream is invalid"),
            }
            if result.bytes_consumed == 0 && result.bytes_written == 0 {
                return if input.is_empty() {
                    Ok(())
                } else {
                    Err("its PNG zlib stream makes no progress")
                };
            }
            if input.is_empty() && result.bytes_written < output.len() {
                return Ok(());
            }
        }
    }

    fn finish(&self) -> Result<(), &'static str> {
        if self.ended && self.expanded == self.expected {
            Ok(())
        } else {
            Err(EXTENT)
        }
    }
}

pub(super) struct State {
    format: Format,
    seen: u16,
    remaining: usize,
    idat: Option<Stream>,
    idat_started: bool,
    idat_closed: bool,
    animation: Option<u32>,
    frames: u32,
    sequence: u64,
    frame: Option<Stream>,
    frame_data_started: bool,
    frame_gap: bool,
}

impl State {
    pub(super) fn new(header: &[u8]) -> Result<Self, &'static str> {
        Ok(Self {
            format: Format::new(header)?,
            seen: 0,
            remaining: MAX_EXPANDED,
            idat: None,
            idat_started: false,
            idat_closed: false,
            animation: None,
            frames: 0,
            sequence: 0,
            frame: None,
            frame_data_started: false,
            frame_gap: false,
        })
    }

    fn stream(&mut self, width: usize, height: usize) -> Result<Stream, &'static str> {
        Stream::new(self.format.extent(width, height)?, &mut self.remaining)
    }

    fn sequence(&mut self, data: &[u8]) -> Result<(), &'static str> {
        if u64::from(be32(data, 0).ok_or(INVALID)?) != self.sequence {
            return Err(INVALID);
        }
        self.sequence += 1;
        Ok(())
    }

    pub(super) fn accept(&mut self, kind: &[u8], data: &[u8]) -> Result<(), &'static str> {
        if kind != b"IDAT" && self.idat_started && !self.idat_closed {
            self.idat.as_ref().ok_or(INVALID)?.finish()?;
            self.idat = None;
            self.idat_closed = true;
        }
        if self.frame.is_some()
            && self.frame_data_started
            && !matches!(kind, b"fdAT" | b"fcTL" | b"IEND")
        {
            self.frame_gap = true;
        }
        let singleton = match kind {
            b"PLTE" => 1,
            b"tRNS" => 2,
            b"cHRM" => 4,
            b"gAMA" => 8,
            b"iCCP" => 16,
            b"sRGB" => 32,
            b"cICP" => 64,
            b"mDCV" => 128,
            b"cLLI" => 256,
            b"acTL" => 512,
            _ => 0,
        };
        if singleton != 0 && self.seen & singleton != 0 {
            return Err("a PNG singleton chunk is repeated");
        }
        match kind {
            b"IHDR" => {}
            b"cHRM" | b"gAMA" | b"iCCP" | b"sRGB" | b"cICP" | b"mDCV" | b"cLLI" => {
                if self.seen & 1 != 0 || self.idat_started {
                    return Err(INVALID);
                }
            }
            b"PLTE" => {
                if self.idat_started
                    || self.seen & 2 != 0
                    || matches!(self.format.colour, 0 | 4)
                    || (self.format.colour == 3 && data.len() / 3 > 1usize << self.format.depth)
                {
                    return Err(INVALID);
                }
            }
            b"tRNS" => {
                if self.idat_started || (self.format.colour == 3 && self.seen & 1 == 0) {
                    return Err(INVALID);
                }
            }
            b"acTL" => {
                if self.idat_started {
                    return Err(INVALID);
                }
                let count = be32(data, 0).ok_or(INVALID)?;
                if count == 0 {
                    return Err(INVALID);
                }
                self.animation = Some(count);
            }
            b"fcTL" => {
                self.frame_data_started = false;
                self.frame_gap = false;
                if self.animation.is_none() {
                    return Err(INVALID);
                }
                if let Some(frame) = self.frame.take() {
                    frame.finish()?;
                }
                self.sequence(data)?;
                let width = be32(data, 4).ok_or(INVALID)? as usize;
                let height = be32(data, 8).ok_or(INVALID)? as usize;
                let x = be32(data, 12).ok_or(INVALID)? as usize;
                let y = be32(data, 16).ok_or(INVALID)? as usize;
                if width == 0
                    || height == 0
                    || x.checked_add(width).is_none_or(|n| n > self.format.width)
                    || y.checked_add(height).is_none_or(|n| n > self.format.height)
                    || data.get(24).is_none_or(|n| *n > 2)
                    || data.get(25).is_none_or(|n| *n > 1)
                {
                    return Err(INVALID);
                }
                self.frames = self.frames.checked_add(1).ok_or(INVALID)?;
                if self.frames > self.animation.ok_or(INVALID)? {
                    return Err(INVALID);
                }
                if !self.idat_started {
                    if self.frames != 1
                        || x != 0
                        || y != 0
                        || width != self.format.width
                        || height != self.format.height
                    {
                        return Err(INVALID);
                    }
                } else {
                    self.frame = Some(self.stream(width, height)?);
                }
            }
            b"IDAT" => {
                if self.idat_closed
                    || self.frame.is_some()
                    || (self.format.colour == 3 && self.seen & 1 == 0)
                {
                    return Err(INVALID);
                }
                if !self.idat_started {
                    self.idat = Some(self.stream(self.format.width, self.format.height)?);
                    self.idat_started = true;
                }
                self.idat.as_mut().ok_or(INVALID)?.feed(data)?;
            }
            b"fdAT" => {
                if self.frame_gap {
                    return Err(INTERLEAVED);
                }
                self.frame_data_started = true;
                if !self.idat_started || self.animation.is_none() {
                    return Err(INVALID);
                }
                self.sequence(data)?;
                self.frame
                    .as_mut()
                    .ok_or(INVALID)?
                    .feed(data.get(4..).ok_or(INVALID)?)?;
            }
            b"IEND" => {
                if !self.idat_started {
                    return Err(INVALID);
                }
                if let Some(frame) = &self.frame {
                    frame.finish()?;
                }
                if self.animation.is_some_and(|count| count != self.frames) {
                    return Err(INVALID);
                }
            }
            _ => {}
        }
        self.seen |= singleton;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_budget_reserves_all_frames_and_checks_exact_bound() {
        assert_eq!(MAX_EXPANDED, 256 * 1024 * 1024);
        let mut remaining = 36;
        assert!(Stream::new(18, &mut remaining).is_ok());
        assert!(Stream::new(18, &mut remaining).is_ok());
        assert_eq!(remaining, 0);
        assert!(Stream::new(1, &mut remaining).is_err());
        assert_eq!(remaining, 0);
    }
    #[test]
    fn adam7_packed_and_sixteen_bit_scanline_extents_are_exact() {
        let header =
            |depth, colour, interlace| [0, 0, 0, 1, 0, 0, 0, 1, depth, colour, 0, 0, interlace];
        for (depth, colour, size) in [
            (1, 0, 2),
            (2, 3, 2),
            (4, 3, 2),
            (8, 2, 4),
            (16, 0, 3),
            (16, 2, 7),
            (16, 4, 5),
            (16, 6, 9),
        ] {
            for interlace in [0, 1] {
                let format = Format::new(&header(depth, colour, interlace)).unwrap();
                assert_eq!(format.extent(1, 1), Ok(size));
            }
        }
    }
    #[test]
    fn streaming_drains_full_buffers_and_refuses_short_or_excess_extents() {
        for n in [18, 32768, 32769, 65536] {
            let raw = vec![0; n];
            let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6);
            let mut budget = n;
            let mut stream = Stream::new(n, &mut budget).unwrap();
            for part in compressed.chunks(1) {
                assert!(stream.feed(part).is_ok());
            }
            assert!(stream.finish().is_ok());
            assert!(stream.feed(b"private").is_err());
            let mut remaining = n + 1;
            let mut short = Stream::new(n + 1, &mut remaining).unwrap();
            assert!(short.feed(&compressed).is_ok());
            assert!(short.finish().is_err());
            let mut remaining = n - 1;
            let mut excess = Stream::new(n - 1, &mut remaining).unwrap();
            assert!(excess.feed(&compressed).is_err());
        }
    }

    #[test]
    fn animation_reserves_one_budget_before_each_frame_stream() {
        let data = crate::uploads::strip::tests::fixture("privacy-apng-included.png");
        let mut state = State::new(&data[16..29]).unwrap();
        state.remaining = 35; // two 18-byte frames need 36, not 18 twice.
        let mut at = 33;
        let mut refused = None;
        while at < data.len() {
            let size = be32(&data, at).unwrap() as usize;
            let kind = &data[at + 4..at + 8];
            if let Err(reason) = state.accept(kind, &data[at + 8..at + 8 + size]) {
                refused = Some(reason);
                break;
            }
            at += 12 + size;
        }
        assert_eq!(refused, Some(BUDGET));
        assert_eq!(
            state.remaining, 17,
            "frame reservation failed before inflation"
        );
    }
}
