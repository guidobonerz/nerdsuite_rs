//! The sound of the Composer: the SID engine (`nerdsuite_core::sid`) renders into the default output
//! of the system, in cpal's audio thread - what the audio worklet of nerdsuite2 does. The view
//! reaches the engine through a lock.

use std::sync::{Arc, Mutex};

use anyhow::{Context as _, Result, anyhow};
use cpal::{
    FromSample, SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use nerdsuite_core::sid::SidEngine;

pub struct Audio {
    engine: Arc<Mutex<SidEngine>>,
    _stream: cpal::Stream,
}

impl Audio {
    pub fn start() -> Result<Audio> {
        let device = cpal::default_host().default_output_device().ok_or_else(|| anyhow!("no sound output"))?;
        let config = device.default_output_config().context("the sound output")?;
        let engine = Arc::new(Mutex::new(SidEngine::new(config.sample_rate().0 as f64)));
        let format = config.sample_format();
        let config: cpal::StreamConfig = config.into();
        let stream = match format {
            cpal::SampleFormat::F32 => stream::<f32>(&device, &config, engine.clone()),
            cpal::SampleFormat::I16 => stream::<i16>(&device, &config, engine.clone()),
            cpal::SampleFormat::U16 => stream::<u16>(&device, &config, engine.clone()),
            cpal::SampleFormat::I32 => stream::<i32>(&device, &config, engine.clone()),
            f => return Err(anyhow!("the sound output wants samples as {f}")),
        }?;
        stream.play().context("starting the sound")?;
        Ok(Audio { engine, _stream: stream })
    }

    pub fn with<R>(&self, f: impl FnOnce(&mut SidEngine) -> R) -> R {
        f(&mut self.engine.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

fn stream<T: SizedSample + FromSample<f32>>(device: &cpal::Device, config: &cpal::StreamConfig, engine: Arc<Mutex<SidEngine>>) -> Result<cpal::Stream> {
    let channels = config.channels as usize;
    let mut mono = Vec::new();
    let stream = device.build_output_stream(
        config,
        move |out: &mut [T], _| {
            mono.resize(out.len() / channels, 0f32);
            engine.lock().unwrap_or_else(|e| e.into_inner()).render(&mut mono);
            for (frame, &s) in out.chunks_mut(channels).zip(&mono) {
                frame.fill(T::from_sample(s));
            }
        },
        |e| eprintln!("sound: {e}"),
        None,
    )?;
    Ok(stream)
}
