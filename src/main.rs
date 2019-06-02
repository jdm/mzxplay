#![feature(nll)]

extern crate env_logger;
extern crate libmzx;
#[macro_use] extern crate log;
extern crate num_traits;
extern crate openmpt;
extern crate rand;
extern crate sdl2;
extern crate time;

use crate::audio::{AudioEngine, MusicCallback};
use crate::game::{InputState, TitleState, PlayState, update_key_states};
use libmzx::{load_world, World, Counters, Renderer, ByteString, Coordinate};
use sdl2::event::Event;
use sdl2::pixels::Color;
use sdl2::render::Canvas;
use sdl2::video::Window;
use std::env;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::exit;
use std::time::Duration;

mod audio;
mod board;
mod game;
mod robot;

struct SdlRenderer<'a> {
    canvas: &'a mut Canvas<Window>,
}

impl<'a> Renderer for SdlRenderer<'a> {
    fn put_pixel(
        &mut self,
        x: usize,
        y: usize,
        r: u8,
        g: u8,
        b: u8,
    ) {
        self.canvas.set_draw_color(Color::RGB(r, g, b));
        self.canvas.draw_point((x as i32, y as i32)).unwrap();
    }

    fn clear(&mut self) {
        self.canvas.set_draw_color(Color::RGB(0, 0, 0));
        self.canvas.clear();
    }
}

enum StateChange {
    PopCurrent(Option<PoppedData>),
    Push(Box<GameState>),
    Replace(Box<GameState>),
}

enum PoppedData {
    MessageBox(robot::RobotId, libmzx::ByteString),
    Scroll(Coordinate<u16>),
}

trait GameState {
    fn init(
        &mut self,
        world: &mut World,
        board_id: &mut usize
    );

    fn popped(
        &mut self,
        world: &mut World,
        board: usize,
        data: PoppedData,
    );

    fn input(
        &mut self,
        event: Event,
        input_state: &mut InputState,
    ) -> Option<StateChange>;

    fn tick(
        &mut self,
        world: &mut World,
        world_path: &Path,
        input_state: &InputState,
        counters: &mut Counters,
        boards: &[ByteString],
        board_id: &mut usize,
    ) -> Option<StateChange>;

    fn render(
        &mut self,
        world: &World,
        board_id: usize,
        canvas: &mut Canvas<Window>,
    );
}

fn update_state(
    states: &mut Vec<Box<GameState>>,
    change: Option<StateChange>,
    world: &mut World,
    board_id: &mut usize,
) {
    match change {
        None => (),
        Some(StateChange::PopCurrent(data)) => {
            let _ = states.pop().expect("no state to pop??");
            if let Some(data) = data {
                if let Some(ref mut current) = states.last_mut() {
                    current.popped(world, *board_id, data);
                }
            }
        }
        Some(StateChange::Push(mut state)) => {
            state.init(world, board_id);
            states.push(state);
        }
        Some(StateChange::Replace(mut state)) => {
            let _ = states.pop().expect("no state to replace??");
            state.init(world, board_id);
            states.push(state);
        }
    }
}

fn run(world_path: &Path, starting_board: Option<usize>, silent: bool) {
    let world_data = match File::open(&world_path) {
        Ok(mut file) => {
            let mut v = vec![];
            file.read_to_end(&mut v).unwrap();
            v
        }
        Err(e) => {
            println!("Error opening {} ({})", world_path.display(), e);
            exit(1)
        }
    };

    let mut world = match load_world(&world_data) {
        Ok(world) => world,
        Err(e) => {
            println!("Error reading {} ({:?})", world_path.display(), e);
            exit(1)
        }
    };

    let world_path = Path::new(&world_path).parent().unwrap();

    let sdl_context = sdl2::init().unwrap();
    let audio_subsystem = sdl_context.audio().unwrap();

    let video_subsystem = sdl_context.video().unwrap();
    let window = video_subsystem.window("revenge of megazeux", 640, 350)
      .position_centered()
      .build()
      .unwrap();

    let mut canvas = window.into_canvas().software().build().unwrap();

    canvas.clear();
    canvas.present();

    canvas.set_draw_color(Color::RGBA(255, 255, 255, 255));

    let music = MusicCallback::new(&world_path, silent);
    let _device = audio::init_sdl(&audio_subsystem, music.clone());

    let mut events = sdl_context.event_pump().unwrap();

    let mut board_id = starting_board.unwrap_or(0);
    music.load_module(&world.boards[board_id].mod_file);
    let game_speed: u64 = 4;

    let mut states = vec![if starting_board.is_none() {
        Box::new(TitleState(music.clone())) as Box<GameState>
    } else {
        Box::new(PlayState::new(music.clone())) as Box<PlayState>
    }];
    states[0].init(&mut world, &mut board_id);

    let mut counters = Counters::new();
    let boards: Vec<_> = world.boards.iter().map(|b| b.title.clone()).collect();

    let mut last_input_state = InputState::default();
    'mainloop: loop {
        let mut input_state = InputState::new_from(&last_input_state);

        let start = time::precise_time_ns();
        for event in events.poll_iter() {
            if let Event::Quit{..} = event {
                break 'mainloop;
            }

            match event {
                Event::KeyDown { ref keycode, .. } =>
                    update_key_states(&mut input_state, *keycode, true),
                Event::KeyUp { ref keycode, .. } =>
                    update_key_states(&mut input_state, *keycode, false),
                _ => (),
            }

            let change = match states.last_mut() {
                Some(state) => state.input(event, &mut input_state),
                None => break 'mainloop,
            };
            update_state(&mut states, change, &mut world, &mut board_id);
        }

        if let Some(state) = states.last_mut() {
            let change = state.tick(
                &mut world,
                &world_path,
                &input_state,
                &mut counters,
                &boards,
                &mut board_id
            );
            update_state(&mut states, change, &mut world, &mut board_id);
        }

        for state in &mut states {
            state.render(
                &world,
                board_id,
                &mut canvas,
            );
        }

        canvas.present();

        let now = time::precise_time_ns();
        let elapsed_ms = (now - start) / 1_000_000;
        let total_ticks = (16 * (game_speed - 1)).checked_sub(elapsed_ms);
        if let Some(diff) = total_ticks {
            ::std::thread::sleep(Duration::from_millis(diff));
        }

        last_input_state = input_state;
    }
}

fn main() {
    env_logger::init();
    let args: Vec<_> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: cargo run /path/to/world.mzx [board id]")
    } else {
        let silent = env::var("SILENT").ok().map_or(false, |s| !s.is_empty());
        run(Path::new(&args[1]), args.get(2).and_then(|a| a.parse().ok()), silent);
    }
}
