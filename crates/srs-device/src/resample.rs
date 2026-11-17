//! New fixed-ratio low-pass decimator. Coefficients are generated once outside
//! callbacks: 97-tap Blackman-windowed sinc, 7 kHz cutoff, 48 -> 16 kHz.
const TAPS: usize = 97;
pub(crate) struct Decimator {
    taps: [f32; TAPS],
    delay: [f32; TAPS],
    position: usize,
    phase: usize,
}
impl Decimator {
    pub fn new() -> Self {
        let mut taps = std::array::from_fn(|i| {
            let x = i as f64 - (TAPS - 1) as f64 / 2.0;
            let cutoff = 7000.0 / 48000.0;
            let sinc = if x == 0.0 {
                2.0 * cutoff
            } else {
                (2.0 * std::f64::consts::PI * cutoff * x).sin() / (std::f64::consts::PI * x)
            };
            let angle = std::f64::consts::TAU * i as f64 / (TAPS - 1) as f64;
            (sinc * (0.42 - 0.5 * angle.cos() + 0.08 * (2.0 * angle).cos())) as f32
        });
        let sum: f32 = taps.iter().sum();
        for tap in &mut taps {
            *tap /= sum;
        }
        Self {
            taps,
            delay: [0.0; TAPS],
            position: 0,
            phase: 0,
        }
    }
    pub fn reset(&mut self) {
        self.delay.fill(0.0);
        self.position = 0;
        self.phase = 0;
    }
    pub fn sample(&mut self, input: f32) -> Option<i16> {
        self.delay[self.position] = input;
        self.position = (self.position + 1) % TAPS;
        self.phase = (self.phase + 1) % 3;
        if self.phase != 0 {
            return None;
        }
        let mut sample = 0.0;
        for (i, coefficient) in self.taps.iter().enumerate() {
            sample += coefficient * self.delay[(self.position + TAPS - 1 - i) % TAPS];
        }
        Some((sample.clamp(-1.0, 1.0) * 32767.0).round() as i16)
    }
}
