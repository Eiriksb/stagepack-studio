//! Making small videos, with the same library's encoder, for tests here and in the other crates.

use std::io::Cursor;

use bytes::Bytes;
use mp4::{AvcConfig, MediaConfig, Mp4Config, Mp4Sample, Mp4Writer, TrackConfig, TrackType};
use openh264::OpenH264API;
use openh264::encoder::{
    BitRate, Encoder, EncoderConfig, FrameRate, FrameType, IntraFramePeriod, QpRange, RateControlMode, VuiConfig,
};
use openh264::formats::YUVBuffer;
use yuvutils_rs::{YuvChromaSubsampling, YuvConversionMode, YuvPlanarImageMut, YuvRange, YuvStandardMatrix, bgra_to_yuv420};

/// What a made video says of the colours its pictures are stored in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MadeColours {
    /// Nothing, so they're read as BT.601 in the usual range.
    Unsaid,
    /// BT.709 in the usual range.
    Bt709,
    /// BT.709 in the full range.
    Bt709Full,
}

/// An H.264 MP4 of `frames` pictures, `picture` giving each as straight BGRA, opaque, `width` by
/// `height`, both even. A picture every second can be started from. Pictures as simple as solid
/// colours come out within 8 levels.
pub fn make_test_video(
    width: u32,
    height: u32,
    frames: u32,
    frames_a_second: u32,
    picture: impl Fn(u32) -> Vec<u8>,
) -> Vec<u8> {
    make_test_video_in(MadeColours::Unsaid, width, height, frames, frames_a_second, picture)
}

/// The same, saying its colours are stored as `colours` says.
pub fn make_test_video_in(
    colours: MadeColours,
    width: u32,
    height: u32,
    frames: u32,
    frames_a_second: u32,
    picture: impl Fn(u32) -> Vec<u8>,
) -> Vec<u8> {
    assert!(width.is_multiple_of(2) && height.is_multiple_of(2) && frames > 0 && frames_a_second > 0);
    let (matrix, range, vui) = match colours {
        MadeColours::Unsaid => (YuvStandardMatrix::Bt601, YuvRange::Limited, None),
        MadeColours::Bt709 => (YuvStandardMatrix::Bt709, YuvRange::Limited, Some(VuiConfig::bt709())),
        MadeColours::Bt709Full => (YuvStandardMatrix::Bt709, YuvRange::Full, Some(VuiConfig::bt709_full())),
    };
    let mut config = EncoderConfig::new()
        .skip_frames(false)
        .max_frame_rate(FrameRate::from_hz(frames_a_second as f32))
        .rate_control_mode(RateControlMode::Off)
        .bitrate(BitRate::from_bps(50_000_000))
        .qp(QpRange::new(10, 20))
        .scene_change_detect(false)
        .intra_frame_period(IntraFramePeriod::from_num_frames(frames_a_second))
        .num_threads(1);
    if let Some(vui) = vui {
        config = config.vui(vui);
    }
    let mut encoder = Encoder::with_api_config(OpenH264API::from_source(), config).expect("an encoder");

    let (mut sps, mut pps) = (None, None);
    let mut samples = Vec::new();
    for i in 0..frames {
        let bgra = picture(i);
        assert_eq!(bgra.len(), (width * height * 4) as usize, "picture {i} isn't {width} by {height}");
        let yuv = to_yuv(&bgra, width, height, range, matrix);
        let stream = encoder.encode(&yuv).expect("an encoded picture");
        let sync = matches!(stream.frame_type(), FrameType::IDR | FrameType::I);
        let mut sample = Vec::new();
        for l in 0..stream.num_layers() {
            let layer = stream.layer(l).expect("a layer");
            for n in 0..layer.nal_count() {
                let unit = strip_start_code(layer.nal_unit(n).expect("a unit"));
                match unit[0] & 0x1F {
                    7 => sps = sps.or_else(|| Some(unit.to_vec())),
                    8 => pps = pps.or_else(|| Some(unit.to_vec())),
                    _ => {
                        sample.extend_from_slice(&(unit.len() as u32).to_be_bytes());
                        sample.extend_from_slice(unit);
                    }
                }
            }
        }
        samples.push((sync, sample));
    }

    // Each frame lasts 1,000 ticks of a clock counting 1,000 times the frame rate.
    let ticks_a_second = frames_a_second * 1000;
    let config = Mp4Config {
        major_brand: str::parse("isom").expect("a brand"),
        minor_version: 512,
        compatible_brands: ["isom", "iso2", "avc1", "mp41"].iter().map(|b| str::parse(b).expect("a brand")).collect(),
        timescale: 1000,
    };
    let mut writer = Mp4Writer::write_start(Cursor::new(Vec::new()), &config).expect("a writer");
    writer
        .add_track(&TrackConfig {
            track_type: TrackType::Video,
            timescale: ticks_a_second,
            language: String::from("und"),
            media_conf: MediaConfig::AvcConfig(AvcConfig {
                width: width as u16,
                height: height as u16,
                seq_param_set: sps.expect("sequence parameters"),
                pic_param_set: pps.expect("picture parameters"),
            }),
        })
        .expect("a track");
    for (i, (is_sync, bytes)) in samples.into_iter().enumerate() {
        let sample =
            Mp4Sample { start_time: i as u64 * 1000, duration: 1000, rendering_offset: 0, is_sync, bytes: Bytes::from(bytes) };
        writer.write_sample(1, &sample).expect("a sample");
    }
    writer.write_end().expect("the index");
    writer.into_writer().into_inner()
}

fn to_yuv(bgra: &[u8], width: u32, height: u32, range: YuvRange, matrix: YuvStandardMatrix) -> YUVBuffer {
    let mut planes = YuvPlanarImageMut::<u8>::alloc(width, height, YuvChromaSubsampling::Yuv420);
    bgra_to_yuv420(&mut planes, bgra, width * 4, range, matrix, YuvConversionMode::Balanced).expect("a conversion");
    let mut yuv = Vec::with_capacity((width * height * 3 / 2) as usize);
    for plane in [&planes.y_plane, &planes.u_plane, &planes.v_plane] {
        yuv.extend_from_slice(plane.borrow());
    }
    YUVBuffer::from_vec(yuv, width as usize, height as usize)
}

fn strip_start_code(unit: &[u8]) -> &[u8] {
    unit.strip_prefix(&[0, 0, 0, 1]).or_else(|| unit.strip_prefix(&[0, 0, 1])).unwrap_or(unit)
}
