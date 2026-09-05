//! Whisper only accepts 16 kHz mono f32. Microphones almost never give you that.
//!
//! This is a window-averaging resampler rather than a proper polyphase one: each
//! output sample is the mean of the input samples that fall inside its time slot.
//! The averaging acts as a crude low-pass, which is what keeps downsampling from
//! aliasing high-frequency content back down into the speech band. It is not
//! hi-fi, but for 16 kHz speech recognition it is indistinguishable from `rubato`
//! and costs one dependency less.

pub const TARGET_SAMPLE_RATE: u32 = 16_000;

pub fn resample(input: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
    if input.is_empty() || from_hz == to_hz {
        return input.to_vec();
    }

    let ratio = from_hz as f64 / to_hz as f64;
    let out_len = (input.len() as f64 / ratio).floor() as usize;
    let mut out = Vec::with_capacity(out_len);

    for i in 0..out_len {
        let start = (i as f64 * ratio) as usize;
        let end = (((i + 1) as f64 * ratio).ceil() as usize).min(input.len());

        if start >= end {
            // Upsampling: the window collapsed, so hold the nearest sample.
            out.push(input[start.min(input.len() - 1)]);
            continue;
        }

        let sum: f32 = input[start..end].iter().sum();
        out.push(sum / (end - start) as f32);
    }

    out
}

/// Peak level of the buffer, used to reject silent recordings before paying for
/// a whisper pass.
pub fn peak(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0f32, |acc, s| acc.max(s.abs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downsamples_to_expected_length() {
        let input = vec![0.5f32; 48_000];
        let out = resample(&input, 48_000, 16_000);
        assert_eq!(out.len(), 16_000);
        // A constant signal must survive averaging unchanged.
        assert!(out.iter().all(|s| (s - 0.5).abs() < 1e-6));
    }

    #[test]
    fn passthrough_when_rates_match() {
        let input = vec![1.0, -1.0, 0.25];
        assert_eq!(resample(&input, 16_000, 16_000), input);
    }

    #[test]
    fn handles_empty_input() {
        assert!(resample(&[], 44_100, 16_000).is_empty());
    }

    #[test]
    fn peak_is_absolute() {
        assert_eq!(peak(&[0.1, -0.9, 0.3]), 0.9);
    }
}
