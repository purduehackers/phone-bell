use std::{
    sync::mpsc::{channel, Receiver, Sender, TryRecvError},
    thread,
    time::Duration,
};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    BuildStreamError, Device, FromSample, Host, Sample, SampleFormat, SampleRate, Stream,
    StreamConfig, StreamError, SupportedStreamConfig,
};

#[macro_export]
macro_rules! create_output_stream {
    ($device:tt, $config:tt, $x:ty, $audio_receiver:tt, $error_sender:tt) => {
        $device.build_output_stream(
            &$config.config(),
            move |data, info| Self::output_stream_data_callback::<$x>(data, info, &$audio_receiver),
            move |error| {
                let _ = $error_sender.send((StreamKind::Outgoing, error));
            },
            None,
        )
    };
}

#[macro_export]
macro_rules! create_input_stream {
    ($device:tt, $config:tt, $x:ty, $audio_receiver:tt, $error_sender:tt) => {
        $device.build_input_stream(
            &$config.config(),
            move |data, info| Self::input_stream_data_callback::<$x>(data, info, &$audio_receiver),
            move |error| {
                let _ = $error_sender.send((StreamKind::Incoming, error));
            },
            None,
        )
    };
}

enum CPALStreamState {
    Nothing,
    Device(Device),
    DeviceConfig(Device, SupportedStreamConfig),
    #[allow(dead_code)] // I may need this later so idk
    DeviceConfigStream(Device, SupportedStreamConfig, Stream),
}

#[derive(Debug)]
pub enum StreamReadError {
    NoStream,
}
#[derive(Debug)]
pub enum StreamWriteError {
    NoStream,
    WriteFailed,
}

pub enum StreamKind {
    Incoming,
    Outgoing,
}

pub struct AudioSystemMarshaller {
    from_input: Receiver<Vec<f32>>,
    to_output: Sender<Vec<f32>>,
}

impl AudioSystemMarshaller {
    pub fn create() -> Self {
        let (input, from_input) = channel();
        let (to_output, output) = channel::<Vec<f32>>();
        thread::spawn(move || {
            let mut audio_system = AudioSystem::create();

            loop {
                let input_ready = audio_system.is_input_ready();
                let output_ready = audio_system.is_output_ready();

                if input_ready {
                    if let Ok(s) = audio_system.read_next_samples() {
                        if !s.is_empty() {
                            input.send(s).unwrap();
                        }
                    }
                }
                if let Ok(r) = output.try_recv() {
                    if output_ready {
                        audio_system.write_next_samples(r.as_slice()).unwrap();
                    }
                }

                if !input_ready || !output_ready {
                    // Audio not set up yet, wait before retrying
                    thread::sleep(Duration::from_secs(2));
                } else {
                    // Sleep ~20ms to collect one Opus frame worth of samples
                    thread::sleep(Duration::from_millis(20));
                }
            }
        });

        Self {
            from_input,
            to_output,
        }
    }

    pub fn send_to_speaker(&self, data: Vec<f32>) {
        self.to_output.send(data).unwrap();
    }

    pub fn try_receive_from_mic(&self) -> Result<Vec<f32>, TryRecvError> {
        self.from_input.try_recv()
    }
}

pub struct AudioSystem {
    cpal_host: Host,

    input_stream: CPALStreamState,
    output_stream: CPALStreamState,

    incoming_audio_buffer: Option<Receiver<f32>>,

    outgoing_audio_buffer: Option<Sender<f32>>,

    pub error_buffer: Receiver<(StreamKind, StreamError)>,
    error_buffer_sender: Sender<(StreamKind, StreamError)>,
}

impl AudioSystem {
    pub fn create() -> AudioSystem {
        let cpal_host = cpal::default_host();

        let (error_buffer_sender, error_buffer) = channel();

        let mut audio_system = AudioSystem {
            cpal_host,

            input_stream: CPALStreamState::Nothing,
            output_stream: CPALStreamState::Nothing,

            incoming_audio_buffer: Option::None,
            outgoing_audio_buffer: Option::None,

            error_buffer,
            error_buffer_sender,
        };

        audio_system.prepare_input();
        audio_system.prepare_output();

        audio_system
    }

    pub fn prepare_input(&mut self) -> bool {
        loop {
            match &self.input_stream {
                CPALStreamState::Nothing => {
                    let Some(device) = self.new_input_device() else {
                        println!("Failed to open audio device!");

                        return false;
                    };

                    self.input_stream = CPALStreamState::Device(device);
                }
                CPALStreamState::Device(device) => {
                    let Some(config) = self.new_input_config(device) else {
                        println!("Failed to get audio config!");

                        return false;
                    };

                    self.input_stream = CPALStreamState::DeviceConfig(device.clone(), config);
                }
                CPALStreamState::DeviceConfig(device, config) => {
                    let (audio_sender, audio_receiver) = channel::<f32>();

                    let Some(stream) = self.new_input_stream(
                        device,
                        config,
                        audio_sender,
                        self.error_buffer_sender.clone(),
                    ) else {
                        println!("Failed to init audio streams!");

                        return false;
                    };

                    let _ = stream.play();

                    self.incoming_audio_buffer = Option::Some(audio_receiver);

                    self.input_stream =
                        CPALStreamState::DeviceConfigStream(device.clone(), config.clone(), stream);
                }
                CPALStreamState::DeviceConfigStream(_, _, _) => {
                    return true;
                }
            }
        }
    }
    pub fn prepare_output(&mut self) -> bool {
        loop {
            match &self.output_stream {
                CPALStreamState::Nothing => {
                    let Some(device) = self.new_output_device() else {
                        println!("Failed to open audio device!");

                        return false;
                    };

                    self.output_stream = CPALStreamState::Device(device);
                }
                CPALStreamState::Device(device) => {
                    let Some(config) = self.new_output_config(device) else {
                        println!("Failed to get audio config!");

                        return false;
                    };

                    self.output_stream = CPALStreamState::DeviceConfig(device.clone(), config);
                }
                CPALStreamState::DeviceConfig(device, config) => {
                    let (audio_sender, audio_receiver) = channel::<f32>();

                    let Some(stream) = self.new_output_stream(
                        device,
                        config,
                        audio_receiver,
                        self.error_buffer_sender.clone(),
                    ) else {
                        println!("Failed to init audio streams!");

                        return false;
                    };

                    let _ = stream.play();

                    self.outgoing_audio_buffer = Option::Some(audio_sender);

                    self.output_stream =
                        CPALStreamState::DeviceConfigStream(device.clone(), config.clone(), stream);
                }
                CPALStreamState::DeviceConfigStream(_, _, _) => {
                    return true;
                }
            }
        }
    }

    fn new_input_device(&self) -> Option<Device> {
        self.cpal_host.default_input_device()
    }
    fn new_output_device(&self) -> Option<Device> {
        self.cpal_host.default_output_device()
    }

    fn pick_config(config_range: cpal::SupportedStreamConfigRange) -> SupportedStreamConfig {
        // Prefer 48kHz (Opus native rate), then clamp to supported range
        let target = SampleRate(48000);
        let min = config_range.min_sample_rate();
        let max = config_range.max_sample_rate();
        let rate = if target.0 >= min.0 && target.0 <= max.0 {
            target
        } else if target.0 < min.0 {
            min
        } else {
            max
        };
        config_range.with_sample_rate(rate)
    }

    fn new_input_config(&self, device: &Device) -> Option<SupportedStreamConfig> {
        match device.supported_input_configs() {
            Ok(supported_configs_range) => {
                let config = supported_configs_range
                    .map(Self::pick_config)
                    .next();
                if config.is_none() {
                    eprintln!("No supported input configurations available for device");
                }
                config
            }
            Err(e) => {
                eprintln!("Error querying supported input configs: {}", e);
                None
            }
        }
    }
    fn new_output_config(&self, device: &Device) -> Option<SupportedStreamConfig> {
        match device.supported_output_configs() {
            Ok(supported_configs_range) => {
                let config = supported_configs_range
                    .map(Self::pick_config)
                    .next();
                if config.is_none() {
                    eprintln!("No supported output configurations available for device");
                }
                config
            }
            Err(e) => {
                eprintln!("Error querying supported output configs: {}", e);
                None
            }
        }
    }

    fn new_input_stream(
        &self,
        device: &Device,
        config: &SupportedStreamConfig,
        audio_sender: Sender<f32>,
        error_sender: Sender<(StreamKind, StreamError)>,
    ) -> Option<Stream> {
        match config.sample_format() {
            SampleFormat::F32 => {
                create_input_stream!(device, config, f32, audio_sender, error_sender)
            }
            SampleFormat::I16 => {
                create_input_stream!(device, config, i16, audio_sender, error_sender)
            }
            SampleFormat::U16 => {
                create_input_stream!(device, config, u16, audio_sender, error_sender)
            }
            SampleFormat::I8 => {
                create_input_stream!(device, config, i8, audio_sender, error_sender)
            }
            SampleFormat::I32 => {
                create_input_stream!(device, config, i32, audio_sender, error_sender)
            }
            SampleFormat::I64 => {
                create_input_stream!(device, config, i64, audio_sender, error_sender)
            }
            SampleFormat::U8 => {
                create_input_stream!(device, config, u8, audio_sender, error_sender)
            }
            SampleFormat::U32 => {
                create_input_stream!(device, config, u32, audio_sender, error_sender)
            }
            SampleFormat::U64 => {
                create_input_stream!(device, config, u64, audio_sender, error_sender)
            }
            SampleFormat::F64 => {
                create_input_stream!(device, config, f64, audio_sender, error_sender)
            }
            _ => Err(BuildStreamError::StreamConfigNotSupported),
        }
        .map_err(|e| {
            eprintln!("Error building input stream: {}", e);
            e
        })
        .ok()
    }
    fn input_stream_data_callback<T: Sample>(
        data: &[T],
        _output_callback_info: &cpal::InputCallbackInfo,
        audio_buffer_reference: &Sender<f32>,
    ) where
        f32: FromSample<T>,
    {
        for sample in data.iter() {
            let _ = audio_buffer_reference.send(sample.to_sample::<f32>());
        }
    }

    fn new_output_stream(
        &self,
        device: &Device,
        config: &SupportedStreamConfig,
        audio_receiver: Receiver<f32>,
        error_sender: Sender<(StreamKind, StreamError)>,
    ) -> Option<Stream> {
        match config.sample_format() {
            SampleFormat::F32 => {
                create_output_stream!(device, config, f32, audio_receiver, error_sender)
            }
            SampleFormat::I16 => {
                create_output_stream!(device, config, i16, audio_receiver, error_sender)
            }
            SampleFormat::U16 => {
                create_output_stream!(device, config, u16, audio_receiver, error_sender)
            }
            SampleFormat::I8 => {
                create_output_stream!(device, config, i8, audio_receiver, error_sender)
            }
            SampleFormat::I32 => {
                create_output_stream!(device, config, i32, audio_receiver, error_sender)
            }
            SampleFormat::I64 => {
                create_output_stream!(device, config, i64, audio_receiver, error_sender)
            }
            SampleFormat::U8 => {
                create_output_stream!(device, config, u8, audio_receiver, error_sender)
            }
            SampleFormat::U32 => {
                create_output_stream!(device, config, u32, audio_receiver, error_sender)
            }
            SampleFormat::U64 => {
                create_output_stream!(device, config, u64, audio_receiver, error_sender)
            }
            SampleFormat::F64 => {
                create_output_stream!(device, config, f64, audio_receiver, error_sender)
            }
            _ => Err(BuildStreamError::StreamConfigNotSupported),
        }
        .map_err(|e| {
            eprintln!("Error building output stream: {}", e);
            e
        })
        .ok()
    }
    fn output_stream_data_callback<T: Sample + FromSample<f32>>(
        data: &mut [T],
        _output_callback_info: &cpal::OutputCallbackInfo,
        audio_buffer_reference: &Receiver<f32>,
    ) {
        for sample in data.iter_mut() {
            match audio_buffer_reference.try_recv() {
                Ok(sample_value) => *sample = T::from_sample(sample_value),
                Err(_) => *sample = Sample::EQUILIBRIUM,
            }
        }
    }

    pub fn is_input_ready(&self) -> bool {
        matches!(&self.input_stream, CPALStreamState::DeviceConfigStream(_, _, _))
    }

    pub fn is_output_ready(&self) -> bool {
        matches!(&self.output_stream, CPALStreamState::DeviceConfigStream(_, _, _))
    }

    pub fn write_next_samples(&mut self, new_samples: &[f32]) -> Result<(), StreamWriteError> {
        self.prepare_output();

        match &self.outgoing_audio_buffer {
            Some(buffer) => {
                for sample in new_samples.iter() {
                    let _ = buffer.send(*sample);
                }
                Ok(())
            }
            None => Err(StreamWriteError::NoStream),
        }
    }

    pub fn read_next_samples(&mut self) -> Result<Vec<f32>, StreamReadError> {
        self.prepare_input();

        match &self.incoming_audio_buffer {
            Some(buffer) => {
                let mut sample_vec = Vec::new();

                for sample in buffer.try_iter() {
                    sample_vec.push(sample);
                }

                Ok(sample_vec)
            }
            None => Err(StreamReadError::NoStream),
        }
    }

    pub fn get_input_config(&self) -> Option<StreamConfig> {
        match &self.input_stream {
            CPALStreamState::Nothing => None,
            CPALStreamState::Device(_) => None,
            CPALStreamState::DeviceConfig(_, config) => Some(config.clone().into()),
            CPALStreamState::DeviceConfigStream(_, config, _) => Some(config.clone().into()),
        }
    }

    pub fn get_output_config(&self) -> Option<StreamConfig> {
        match &self.output_stream {
            CPALStreamState::Nothing => None,
            CPALStreamState::Device(_) => None,
            CPALStreamState::DeviceConfig(_, config) => Some(config.clone().into()),
            CPALStreamState::DeviceConfigStream(_, config, _) => Some(config.clone().into()),
        }
    }
}
