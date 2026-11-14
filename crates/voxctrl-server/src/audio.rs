//! Turning an uploaded WAV file into what the models take: 16 kHz mono `f32`.
//!
//! The VoxCtrl client sends exactly that as 16-bit PCM, so for it this is a
//! header skip and a scale. Other OpenAI-compatible clients send whatever they
//! recorded, so other bit depths, channel counts and sample rates are
//! converted rather than refused. Compressed formats (MP3, Ogg, FLAC, M4A) are
//! not decoded: the error says so instead of transcribing noise.

use thiserror::Error;

/// The sample rate every backend expects.
pub const TARGET_RATE: u32 = 16_000;

#[derive(Debug, Error, PartialEq)]
pub enum AudioError {
    #[error("The uploaded file is not WAV audio. Send 16 kHz mono 16-bit PCM WAV; compressed formats such as MP3, Ogg, FLAC and M4A are not supported.")]
    NotWav,
    #[error("The WAV file has no 'fmt ' chunk before its audio data.")]
    MissingFormat,
    #[error("The WAV file has no 'data' chunk.")]
    MissingData,
    #[error("Unsupported WAV encoding (format tag {tag}, {bits} bits). Send PCM or IEEE float.")]
    UnsupportedEncoding { tag: u16, bits: u16 },
    #[error("The WAV header is invalid: {0}.")]
    BadHeader(&'static str),
}

#[derive(Debug, Clone, Copy)]
struct Format {
    tag: u16,
    channels: u16,
    rate: u32,
    bits: u16,
}

const TAG_PCM: u16 = 1;
const TAG_FLOAT: u16 = 3;
const TAG_EXTENSIBLE: u16 = 0xFFFE;

fn u16_at(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// Decode a WAV file to 16 kHz mono samples in `-1.0..=1.0`.
///
/// A `data` chunk whose declared size runs past the end of the file is read to
/// the end instead. That is not leniency for its own sake: a client that
/// streams audio while it is still being recorded cannot know the length when
/// it writes the header, and VoxCtrl's own client declares `0xFFFFFFFF`.
pub fn decode_wav(bytes: &[u8]) -> Result<Vec<f32>, AudioError> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(AudioError::NotWav);
    }

    let mut format: Option<Format> = None;
    let mut pos = 12usize;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let declared = u32_at(bytes, pos + 4) as usize;
        let body = pos + 8;
        let available = bytes.len() - body;

        if id == b"data" {
            let format = format.ok_or(AudioError::MissingFormat)?;
            let len = declared.min(available);
            return convert(&bytes[body..body + len], format);
        }

        if declared > available {
            return Err(AudioError::BadHeader("a chunk is longer than the file"));
        }
        if id == b"fmt " {
            format = Some(parse_format(&bytes[body..body + declared])?);
        }
        // Chunks are padded to an even length.
        pos = body + declared + (declared & 1);
    }
    Err(AudioError::MissingData)
}

fn parse_format(fmt: &[u8]) -> Result<Format, AudioError> {
    if fmt.len() < 16 {
        return Err(AudioError::BadHeader("the 'fmt ' chunk is too short"));
    }
    let mut tag = u16_at(fmt, 0);
    let channels = u16_at(fmt, 2);
    let rate = u32_at(fmt, 4);
    let bits = u16_at(fmt, 14);
    // WAVE_FORMAT_EXTENSIBLE keeps the real tag in the first two bytes of the
    // sub-format GUID.
    if tag == TAG_EXTENSIBLE && fmt.len() >= 26 {
        tag = u16_at(fmt, 24);
    }
    if channels == 0 {
        return Err(AudioError::BadHeader("it declares zero channels"));
    }
    if rate == 0 {
        return Err(AudioError::BadHeader("it declares a sample rate of zero"));
    }
    Ok(Format { tag, channels, rate, bits })
}

fn convert(data: &[u8], format: Format) -> Result<Vec<f32>, AudioError> {
    let bytes_per_sample = match (format.tag, format.bits) {
        (TAG_PCM, 8) => 1,
        (TAG_PCM, 16) => 2,
        (TAG_PCM, 24) => 3,
        (TAG_PCM, 32) | (TAG_FLOAT, 32) => 4,
        (TAG_FLOAT, 64) => 8,
        (tag, bits) => return Err(AudioError::UnsupportedEncoding { tag, bits }),
    };
    let channels = format.channels as usize;
    let frame = bytes_per_sample * channels;

    // A trailing partial frame (a stream cut mid-sample) is dropped.
    let mut mono = Vec::with_capacity(data.len() / frame);
    for frame_bytes in data.chunks_exact(frame) {
        let mut sum = 0.0f32;
        for sample in frame_bytes.chunks_exact(bytes_per_sample) {
            sum += match (format.tag, format.bits) {
                (TAG_PCM, 8) => (sample[0] as f32 - 128.0) / 128.0,
                (TAG_PCM, 16) => i16::from_le_bytes([sample[0], sample[1]]) as f32 / 32768.0,
                (TAG_PCM, 24) => {
                    // Sign-extend by placing the three bytes at the top of an i32.
                    let v = i32::from_le_bytes([0, sample[0], sample[1], sample[2]]) >> 8;
                    v as f32 / 8_388_608.0
                }
                (TAG_PCM, 32) => {
                    i32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]) as f32 / 2_147_483_648.0
                }
                (TAG_FLOAT, 32) => f32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]),
                _ => f64::from_le_bytes([
                    sample[0], sample[1], sample[2], sample[3], sample[4], sample[5], sample[6], sample[7],
                ]) as f32,
            };
        }
        mono.push(sum / channels as f32);
    }

    Ok(resample_to_target(&mono, format.rate))
}

/// Linear-interpolation resample to [`TARGET_RATE`]. Speech models are
/// forgiving of this; it exists so a 44.1 or 48 kHz upload transcribes at all,
/// not to be a mastering-grade converter. Audio already at the target rate —
/// everything the VoxCtrl client sends — is returned untouched.
fn resample_to_target(samples: &[f32], rate: u32) -> Vec<f32> {
    if rate == TARGET_RATE || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = rate as f64 / TARGET_RATE as f64;
    let out_len = ((samples.len() as f64) / ratio).floor() as usize;
    let last = samples.len() - 1;
    (0..out_len)
        .map(|i| {
            let src = i as f64 * ratio;
            let lo = (src.floor() as usize).min(last);
            let hi = (lo + 1).min(last);
            let frac = (src - lo as f64) as f32;
            samples[lo] + (samples[hi] - samples[lo]) * frac
        })
        .collect()
}

/// Root-mean-square level of `audio`; 0 for an empty buffer.
pub fn rms(audio: &[f32]) -> f32 {
    if audio.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = audio.iter().map(|&s| s * s).sum();
    (sum_sq / audio.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A WAV file with the given format and raw data bytes. `declared` is the
    /// size written in the data chunk header (and derived into the RIFF size).
    fn wav(tag: u16, channels: u16, rate: u32, bits: u16, data: &[u8], declared: u32) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&declared.saturating_add(36).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        let block = channels * (bits / 8);
        out.extend_from_slice(&(rate * block as u32).to_le_bytes());
        out.extend_from_slice(&block.to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&declared.to_le_bytes());
        out.extend_from_slice(data);
        out
    }

    fn pcm16(samples: &[i16]) -> Vec<u8> {
        samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    #[test]
    fn decodes_16k_mono_pcm16() {
        let data = pcm16(&[0, 16384, -16384, 32767, -32768]);
        let out = decode_wav(&wav(1, 1, 16_000, 16, &data, data.len() as u32)).unwrap();
        assert_eq!(out.len(), 5);
        assert_eq!(out[0], 0.0);
        assert!((out[1] - 0.5).abs() < 1e-6);
        assert!((out[2] + 0.5).abs() < 1e-6);
        assert!(out[3] > 0.999 && out[3] <= 1.0);
        assert_eq!(out[4], -1.0);
    }

    #[test]
    fn matches_the_clients_own_encoder() {
        // The round trip the app's remote engine relies on.
        let samples = [0.0f32, 0.25, -0.25, 0.9];
        let wav = voxctrl_inference::remote_openai::encode_wav(&samples);
        let out = decode_wav(&wav).unwrap();
        assert_eq!(out.len(), samples.len());
        for (a, b) in out.iter().zip(samples) {
            assert!((a - b).abs() < 1e-3, "{a} vs {b}");
        }
    }

    #[test]
    fn reads_a_streamed_file_with_unknown_length_to_the_end() {
        // What the client sends while recording: both sizes are 0xFFFFFFFF.
        let mut file = voxctrl_inference::remote_openai::create_streaming_wav_header().to_vec();
        file.extend_from_slice(&pcm16(&[100, 200, 300]));
        assert_eq!(decode_wav(&file).unwrap().len(), 3);
    }

    #[test]
    fn drops_a_trailing_partial_sample() {
        let mut data = pcm16(&[1000, 2000]);
        data.push(0x7f); // half a sample: the stream was cut mid-frame
        let out = decode_wav(&wav(1, 1, 16_000, 16, &data, u32::MAX)).unwrap();
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn downmixes_stereo_to_mono() {
        let data = pcm16(&[16384, 0, -16384, -16384]);
        let out = decode_wav(&wav(1, 2, 16_000, 16, &data, data.len() as u32)).unwrap();
        assert_eq!(out.len(), 2);
        assert!((out[0] - 0.25).abs() < 1e-6);
        assert!((out[1] + 0.5).abs() < 1e-6);
    }

    #[test]
    fn resamples_48k_to_16k() {
        let data = pcm16(&vec![8192i16; 48_000]);
        let out = decode_wav(&wav(1, 1, 48_000, 16, &data, data.len() as u32)).unwrap();
        assert_eq!(out.len(), 16_000);
        assert!(out.iter().all(|s| (s - 0.25).abs() < 1e-6));
    }

    #[test]
    fn resamples_8k_up_to_16k() {
        let data = pcm16(&vec![8192i16; 8_000]);
        let out = decode_wav(&wav(1, 1, 8_000, 16, &data, data.len() as u32)).unwrap();
        assert_eq!(out.len(), 16_000);
        assert!(out.iter().all(|s| (s - 0.25).abs() < 1e-6));
    }

    #[test]
    fn decodes_float32_and_8_bit_and_24_bit() {
        let f: Vec<u8> = [0.5f32, -0.5].iter().flat_map(|s| s.to_le_bytes()).collect();
        assert_eq!(decode_wav(&wav(3, 1, 16_000, 32, &f, f.len() as u32)).unwrap(), vec![0.5, -0.5]);

        let out = decode_wav(&wav(1, 1, 16_000, 8, &[128, 255, 0], 3)).unwrap();
        assert_eq!(out[0], 0.0);
        assert!(out[1] > 0.99);
        assert_eq!(out[2], -1.0);

        // 0x400000 = half scale; 0xC00000 = negative half scale.
        let out = decode_wav(&wav(1, 1, 16_000, 24, &[0, 0, 0x40, 0, 0, 0xC0], 6)).unwrap();
        assert!((out[0] - 0.5).abs() < 1e-6);
        assert!((out[1] + 0.5).abs() < 1e-6);
    }

    #[test]
    fn skips_unknown_chunks_before_the_data() {
        let data = pcm16(&[1, 2, 3]);
        let full = wav(1, 1, 16_000, 16, &data, data.len() as u32);
        // Splice an odd-length LIST chunk (padded to even) between fmt and data.
        let mut file = full[..36].to_vec();
        file.extend_from_slice(b"LIST");
        file.extend_from_slice(&3u32.to_le_bytes());
        file.extend_from_slice(&[1, 2, 3, 0]);
        file.extend_from_slice(&full[36..]);
        assert_eq!(decode_wav(&file).unwrap().len(), 3);
    }

    #[test]
    fn rejects_what_it_cannot_decode() {
        assert_eq!(decode_wav(b"ID3\x03not a wav file at all"), Err(AudioError::NotWav));
        assert_eq!(decode_wav(b""), Err(AudioError::NotWav));
        assert_eq!(decode_wav(b"RIFF\x04\0\0\0WAVE"), Err(AudioError::MissingData));
        // A data chunk with no fmt chunk before it.
        let mut no_fmt = b"RIFF\x0c\0\0\0WAVEdata".to_vec();
        no_fmt.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(decode_wav(&no_fmt), Err(AudioError::MissingFormat));
        // mu-law (tag 7) is neither PCM nor float.
        assert_eq!(
            decode_wav(&wav(7, 1, 8_000, 8, &[0; 4], 4)),
            Err(AudioError::UnsupportedEncoding { tag: 7, bits: 8 })
        );
    }

    #[test]
    fn empty_data_is_empty_audio() {
        assert!(decode_wav(&wav(1, 1, 16_000, 16, &[], 0)).unwrap().is_empty());
    }

    #[test]
    fn rms_of_silence_and_of_a_constant() {
        assert_eq!(rms(&[]), 0.0);
        assert_eq!(rms(&[0.0; 100]), 0.0);
        assert!((rms(&[0.5; 100]) - 0.5).abs() < 1e-6);
    }
}
