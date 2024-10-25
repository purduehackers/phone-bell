use std::sync::mpsc::{channel, Receiver, Sender};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    BuildStreamError, Device, FromSample, Host, Sample, SampleFormat, Stream, StreamConfig,
    StreamError, SupportedStreamConfig,
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

pub enum StreamReadError {
    NoStream,
}
pub enum StreamWriteError {
    NoStream,
    WriteFailed,
}

pub enum StreamKind {
    Incoming,
    Outgoing,
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

    fn new_input_config(&self, device: &Device) -> Option<SupportedStreamConfig> {
        match device.supported_input_configs() {
            Ok(mut supported_configs_range) => supported_configs_range
                .next()
                .map(|supported_config| supported_config.with_max_sample_rate()),
            Err(_) => None,
        }
    }
    fn new_output_config(&self, device: &Device) -> Option<SupportedStreamConfig> {
        match device.supported_output_configs() {
            Ok(mut supported_configs_range) => supported_configs_range
                .next()
                .map(|supported_config| supported_config.with_max_sample_rate()),
            Err(_) => None,
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

                for sample in buffer.iter() {
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
