use std::sync::mpsc::{self, Receiver, Sender};

use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    BuildStreamError, Device, FromSample, Host, Sample, SampleFormat, SampleRate, Stream,
    StreamConfig, StreamError, SupportedStreamConfig,
};
use tokio::sync::watch;

use crate::config::SAMPLE_RATE;

#[macro_export]
macro_rules! create_output_stream {
    ($device:tt, $config:tt, $x:ty, $audio_receiver:tt, $mute_watcher:tt, $error_sender:tt, $config_copy:tt) => {
        $device.build_output_stream(
            &$config.config(),
            move |data, info| {
                Self::output_stream_data_callback::<$x>(
                    data,
                    info,
                    &$audio_receiver,
                    &mut $mute_watcher,
                    &$config_copy,
                )
            },
            move |error| {
                let _ = $error_sender.send((StreamKind::Outgoing, error));
            },
            None,
        )
    };
}

#[macro_export]
macro_rules! create_input_stream {
    ($device:tt, $config:tt, $x:ty, $audio_receiver:tt, $mute_watcher:tt, $error_sender:tt, $config_copy:tt) => {
        $device.build_input_stream(
            &$config.config(),
            move |data, info| {
                Self::input_stream_data_callback::<$x>(
                    data,
                    info,
                    &$audio_receiver,
                    &mut $mute_watcher,
                    &$config_copy,
                )
            },
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

pub struct AudioMixer {
    from_inputs: Receiver<MixerMessage>,
    to_output: Sender<Vec<f32>>,
}

pub enum MixerMessage {
    Open(i64),
    Samples(i64, u16, Vec<f32>),
    Close(i64),
}

impl AudioMixer {
    pub fn create() -> (Self, mpsc::Sender<MixerMessage>, mpsc::Receiver<Vec<f32>>) {
        let (mixer_input, from_inputs) = mpsc::channel();
        let (to_output, mixer_output) = mpsc::channel();

        (
            Self {
                from_inputs,
                to_output,
            },
            mixer_input,
            mixer_output,
        )
    }

    pub fn run(&mut self) {
        // TODO: Resequence
        // let mut channel_map = HashMap::<i64, (u16, Vec<f32>)>::new();

        // loop {
        //     let Ok(mixer_message) = self.from_inputs.recv() else {
        //         continue;
        //     };

        //     match mixer_message {
        //         MixerMessage::Open(channel_number) => {
        //             channel_map.insert(channel_number, (0, Vec::new()));
        //         }
        //         MixerMessage::Samples(channel_number, sequence_number, samples) => {
        //             let (base_sample, sample_buffer) = channel_map
        //                 .entry(channel_number)
        //                 .or_insert_with(|| (0, Vec::new()));
        //         }
        //         MixerMessage::Close(channel_number) => {
        //             let _ = channel_map.remove(&channel_number);
        //         }
        //     }
        // }

        // ! this code is for testing purposes only
        loop {
            let Ok(mixer_message) = self.from_inputs.recv() else {
                continue;
            };

            match mixer_message {
                MixerMessage::Open(_) => {}
                MixerMessage::Samples(_, _, samples) => {
                    let _ = self.to_output.send(samples);
                }
                MixerMessage::Close(_) => {}
            }
        }
    }
}

pub struct AudioSystem {
    cpal_host: Host,

    input_stream: CPALStreamState,
    output_stream: CPALStreamState,

    incoming_audio_buffer: Option<Receiver<f32>>,

    outgoing_audio_buffer: Option<Sender<f32>>,
    outgoing_sample_buffer: Vec<f32>,

    pub error_buffer: Receiver<(StreamKind, StreamError)>,
    error_buffer_sender: Sender<(StreamKind, StreamError)>,

    mute_watcher: watch::Sender<bool>,
}

impl AudioSystem {
    pub fn create() -> AudioSystem {
        let cpal_host = cpal::default_host();

        let (error_buffer_sender, error_buffer) = mpsc::channel();

        let (mute_watcher, _) = watch::channel(true);

        let mut audio_system = AudioSystem {
            cpal_host,

            input_stream: CPALStreamState::Nothing,
            output_stream: CPALStreamState::Nothing,

            incoming_audio_buffer: Option::None,
            outgoing_audio_buffer: Option::None,
            outgoing_sample_buffer: Vec::new(),

            error_buffer,
            error_buffer_sender,

            mute_watcher,
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
                    let (audio_sender, audio_receiver) = mpsc::channel::<f32>();

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
                    let (audio_sender, audio_receiver) = mpsc::channel::<f32>();

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
                .map(|supported_config| supported_config.with_sample_rate(SampleRate(SAMPLE_RATE))),
            Err(_) => None,
        }
    }
    fn new_output_config(&self, device: &Device) -> Option<SupportedStreamConfig> {
        match device.supported_output_configs() {
            Ok(mut supported_configs_range) => supported_configs_range
                .next()
                .map(|supported_config| supported_config.with_sample_rate(SampleRate(SAMPLE_RATE))),
            Err(_) => None,
        }
    }

    fn new_input_stream(
        &self,
        device: &Device,
        config: &SupportedStreamConfig,
        audio_sender: mpsc::Sender<f32>,
        error_sender: mpsc::Sender<(StreamKind, StreamError)>,
    ) -> Option<Stream> {
        let config_copy = config.clone();

        let mut mute_watcher = self.mute_watcher.subscribe();

        match config.sample_format() {
            SampleFormat::F32 => {
                create_input_stream!(
                    device,
                    config,
                    f32,
                    audio_sender,
                    mute_watcher,
                    error_sender,
                    config_copy
                )
            }
            SampleFormat::I16 => {
                create_input_stream!(
                    device,
                    config,
                    i16,
                    audio_sender,
                    mute_watcher,
                    error_sender,
                    config_copy
                )
            }
            SampleFormat::U16 => {
                create_input_stream!(
                    device,
                    config,
                    u16,
                    audio_sender,
                    mute_watcher,
                    error_sender,
                    config_copy
                )
            }
            SampleFormat::I8 => {
                create_input_stream!(
                    device,
                    config,
                    i8,
                    audio_sender,
                    mute_watcher,
                    error_sender,
                    config_copy
                )
            }
            SampleFormat::I32 => {
                create_input_stream!(
                    device,
                    config,
                    i32,
                    audio_sender,
                    mute_watcher,
                    error_sender,
                    config_copy
                )
            }
            SampleFormat::I64 => {
                create_input_stream!(
                    device,
                    config,
                    i64,
                    audio_sender,
                    mute_watcher,
                    error_sender,
                    config_copy
                )
            }
            SampleFormat::U8 => {
                create_input_stream!(
                    device,
                    config,
                    u8,
                    audio_sender,
                    mute_watcher,
                    error_sender,
                    config_copy
                )
            }
            SampleFormat::U32 => {
                create_input_stream!(
                    device,
                    config,
                    u32,
                    audio_sender,
                    mute_watcher,
                    error_sender,
                    config_copy
                )
            }
            SampleFormat::U64 => {
                create_input_stream!(
                    device,
                    config,
                    u64,
                    audio_sender,
                    mute_watcher,
                    error_sender,
                    config_copy
                )
            }
            SampleFormat::F64 => {
                create_input_stream!(
                    device,
                    config,
                    f64,
                    audio_sender,
                    mute_watcher,
                    error_sender,
                    config_copy
                )
            }
            _ => Err(BuildStreamError::StreamConfigNotSupported),
        }
        .ok()
    }
    fn input_stream_data_callback<T: Sample>(
        data: &[T],
        _output_callback_info: &cpal::InputCallbackInfo,
        audio_buffer_reference: &mpsc::Sender<f32>,
        mute_watcher: &mut watch::Receiver<bool>,
        config: &SupportedStreamConfig,
    ) where
        f32: FromSample<T>,
    {
        let is_mute = *(mute_watcher.borrow_and_update());

        for sample in data.iter().step_by(config.channels() as usize) {
            let _ = audio_buffer_reference.send(if is_mute {
                Sample::EQUILIBRIUM
            } else {
                sample.to_sample::<f32>()
            });
        }
    }

    fn new_output_stream(
        &self,
        device: &Device,
        config: &SupportedStreamConfig,
        audio_receiver: mpsc::Receiver<f32>,
        error_sender: mpsc::Sender<(StreamKind, StreamError)>,
    ) -> Option<Stream> {
        let config_copy = config.clone();

        let mut mute_watcher = self.mute_watcher.subscribe();

        match config.sample_format() {
            SampleFormat::F32 => {
                create_output_stream!(
                    device,
                    config,
                    f32,
                    audio_receiver,
                    mute_watcher,
                    error_sender,
                    config_copy
                )
            }
            SampleFormat::I16 => {
                create_output_stream!(
                    device,
                    config,
                    i16,
                    audio_receiver,
                    mute_watcher,
                    error_sender,
                    config_copy
                )
            }
            SampleFormat::U16 => {
                create_output_stream!(
                    device,
                    config,
                    u16,
                    audio_receiver,
                    mute_watcher,
                    error_sender,
                    config_copy
                )
            }
            SampleFormat::I8 => {
                create_output_stream!(
                    device,
                    config,
                    i8,
                    audio_receiver,
                    mute_watcher,
                    error_sender,
                    config_copy
                )
            }
            SampleFormat::I32 => {
                create_output_stream!(
                    device,
                    config,
                    i32,
                    audio_receiver,
                    mute_watcher,
                    error_sender,
                    config_copy
                )
            }
            SampleFormat::I64 => {
                create_output_stream!(
                    device,
                    config,
                    i64,
                    audio_receiver,
                    mute_watcher,
                    error_sender,
                    config_copy
                )
            }
            SampleFormat::U8 => {
                create_output_stream!(
                    device,
                    config,
                    u8,
                    audio_receiver,
                    mute_watcher,
                    error_sender,
                    config_copy
                )
            }
            SampleFormat::U32 => {
                create_output_stream!(
                    device,
                    config,
                    u32,
                    audio_receiver,
                    mute_watcher,
                    error_sender,
                    config_copy
                )
            }
            SampleFormat::U64 => {
                create_output_stream!(
                    device,
                    config,
                    u64,
                    audio_receiver,
                    mute_watcher,
                    error_sender,
                    config_copy
                )
            }
            SampleFormat::F64 => {
                create_output_stream!(
                    device,
                    config,
                    f64,
                    audio_receiver,
                    mute_watcher,
                    error_sender,
                    config_copy
                )
            }
            _ => Err(BuildStreamError::StreamConfigNotSupported),
        }
        .ok()
    }
    fn output_stream_data_callback<T: Sample + FromSample<f32>>(
        data: &mut [T],
        _output_callback_info: &cpal::OutputCallbackInfo,
        audio_buffer_reference: &mpsc::Receiver<f32>,
        mute_watcher: &mut watch::Receiver<bool>,
        config: &SupportedStreamConfig,
    ) {
        let is_mute = *(mute_watcher.borrow_and_update());

        for sample in data.iter_mut().step_by(config.channels() as usize) {
            match audio_buffer_reference.try_recv() {
                Ok(sample_value) => {
                    *sample = if is_mute {
                        Sample::EQUILIBRIUM
                    } else {
                        T::from_sample(sample_value)
                    }
                }
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

    pub fn read_next_frames(&mut self) -> Result<Vec<Vec<f32>>, StreamReadError> {
        const SAMPLE_RATE_PER_MILLISECOND: f32 = (SAMPLE_RATE / 1000) as f32;

        const FRAME_LENGTH_25: usize = (SAMPLE_RATE_PER_MILLISECOND * 2.5) as usize;
        const FRAME_LENGTH_50: usize = (SAMPLE_RATE_PER_MILLISECOND * 5.0) as usize;
        const FRAME_LENGTH_100: usize = (SAMPLE_RATE_PER_MILLISECOND * 10.0) as usize;
        const FRAME_LENGTH_200: usize = (SAMPLE_RATE_PER_MILLISECOND * 20.0) as usize;
        const FRAME_LENGTH_400: usize = (SAMPLE_RATE_PER_MILLISECOND * 40.0) as usize;
        const FRAME_LENGTH_600: usize = (SAMPLE_RATE_PER_MILLISECOND * 60.0) as usize;

        self.prepare_input();

        match &self.incoming_audio_buffer {
            Some(buffer) => {
                while let Ok(sample) = buffer.try_recv() {
                    self.outgoing_sample_buffer.push(sample);
                }

                let mut frames = Vec::new();

                let mut available_samples = self.outgoing_sample_buffer.len();

                while available_samples >= FRAME_LENGTH_25 {
                    if available_samples >= FRAME_LENGTH_600 {
                        frames.push(
                            self.outgoing_sample_buffer
                                .drain(0..FRAME_LENGTH_600)
                                .collect(),
                        );
                    } else if available_samples >= FRAME_LENGTH_400 {
                        frames.push(
                            self.outgoing_sample_buffer
                                .drain(0..FRAME_LENGTH_400)
                                .collect(),
                        );
                    } else if available_samples >= FRAME_LENGTH_200 {
                        frames.push(
                            self.outgoing_sample_buffer
                                .drain(0..FRAME_LENGTH_200)
                                .collect(),
                        );
                    } else if available_samples >= FRAME_LENGTH_100 {
                        frames.push(
                            self.outgoing_sample_buffer
                                .drain(0..FRAME_LENGTH_100)
                                .collect(),
                        );
                    } else if available_samples >= FRAME_LENGTH_50 {
                        frames.push(
                            self.outgoing_sample_buffer
                                .drain(0..FRAME_LENGTH_50)
                                .collect(),
                        );
                    } else {
                        frames.push(
                            self.outgoing_sample_buffer
                                .drain(0..FRAME_LENGTH_25)
                                .collect(),
                        );
                    }

                    available_samples = self.outgoing_sample_buffer.len();
                }

                Ok(frames)
            }
            None => Err(StreamReadError::NoStream),
        }
    }

    pub fn set_mute(&mut self, mute: bool) {
        let _ = self.mute_watcher.send(mute);
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
