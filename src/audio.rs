use libmzx::audio::AudioEngine;
use openmpt::module::{Module, Logger};
use sdl2::AudioSubsystem;
use sdl2::audio::{AudioDevice, AudioSpecDesired, AudioCallback};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub fn init_sdl(
    audio_subsystem: &AudioSubsystem,
    music: MusicCallback
) -> AudioDevice<impl AudioCallback> {
    let desired_spec = AudioSpecDesired {
        freq: Some(44100),
        channels: Some(2),
        samples: None,
    };
    let device = audio_subsystem.open_playback(None, &desired_spec, move |spec| {
        music.0.lock().unwrap().rate = spec.freq;
        music
    }).unwrap();
    device.resume();
    device
}

pub struct MusicData {
    world_path: PathBuf,
    rate: i32,
    current_module: Option<(String, Module)>,
    new_position: Option<i32>,
    silent: bool,
}

impl MusicData {
    fn new(world_path: &Path, silent: bool) -> MusicData {
        MusicData {
            world_path: world_path.to_owned(),
            rate: 0,
            current_module: None,
            new_position: None,
            silent,
        }
    }
}

#[derive(Clone)]
pub struct MusicCallback(Arc<Mutex<MusicData>>);
unsafe impl Send for MusicCallback {}

impl MusicCallback {
    pub fn new(world_path: &Path, silent: bool) -> MusicCallback {
        MusicCallback(Arc::new(Mutex::new(MusicData::new(world_path, silent))))
    }
}

impl AudioEngine for MusicCallback {
    fn mod_fade_in(&self, file_path: &str) {
        // TODO: actually fade.
        self.load_module(file_path);
    }

    fn load_module(&self, file_path: &str) {
        let file_path = file_path.to_ascii_lowercase();
        let mut data = self.0.lock().unwrap();
        if data.current_module.as_ref().map_or(false, |(current, _)| current == &file_path) {
            return;
        }
        let module_path = Path::join(&data.world_path, &file_path);
        let module_data = match File::open(&module_path) {
            Ok(mut file) => {
                let mut v = vec![];
                match file.read_to_end(&mut v) {
                    Ok(_) => match Module::create_from_memory(&v[..], Logger::StdErr, &[]) {
                        Ok(m) => Some((file_path, m)),
                        Err(()) => {
                            eprintln!("Error loading {}", module_path.display());
                            None
                        }
                    },
                    Err(e) => {
                        if !file_path.is_empty() {
                            eprintln!("Error opening {} ({})", module_path.display(), e);
                        }
                        None
                    }
                }
            }
            Err(e) => {
                eprintln!("Error opening {} ({})", module_path.display(), e);
                None
            }
        };
        if !data.silent {
            data.current_module = module_data;
        }
        data.new_position = None;
    }

    fn end_module(&self) {
        let mut data = self.0.lock().unwrap();
        data.current_module = None;
    }

    fn mod_fade_out(&self) {
        // TODO: actually fade.
        self.end_module();
    }

    fn set_mod_order(&self, order: i32) {
        let mut data = self.0.lock().unwrap();
        data.new_position = Some(order);
    }
}

impl AudioCallback for MusicCallback {
    type Channel = f32;

    fn callback(&mut self, out: &mut [f32]) {
        let mut data = self.0.lock().unwrap();
        let rate = data.rate;
        let position = data.new_position.take();
        if let Some((_, ref mut module)) = data.current_module {
            if let Some(new_position) = position {
                module.set_position_order_row(new_position, 0);
            }
            module.read_interleaved_float_stereo(rate, out);
        } else {
            for i in out {
                *i = 0.;
            }
        }
    }
}
