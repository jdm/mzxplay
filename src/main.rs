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
use crate::game::{InputState, TitleState, PlayState};
use libmzx::{load_world, World, Counters, Renderer};
use sdl2::event::Event;
use sdl2::pixels::Color;
use sdl2::render::Canvas;
use sdl2::video::Window;
use std::cell::Cell;
use std::env;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::exit;
use std::rc::Rc;
use std::time::Duration;

mod audio;
mod board;
#[cfg(target_os = "emscripten")]
pub mod emscripten;
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

fn run(world_path: &Path, starting_board: Option<usize>) {
    println!("run!");
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

    let music = MusicCallback::new(&world_path);
    let _device = audio::init_sdl(&audio_subsystem, music.clone());

    let mut states = vec![if starting_board.is_none() {
        Box::new(TitleState(music.clone())) as Box<GameState>
    } else {
        Box::new(PlayState(music.clone())) as Box<PlayState>
    }];

    let mut events = sdl_context.event_pump().unwrap();

    let mut board_id = starting_board.unwrap_or(0);
    music.load_module(&world.boards[board_id].mod_file);
    let game_speed: u64 = 4;

    let mut counters = Counters::new();

    let should_exit = Rc::new(Cell::new(false));
    let should_exit2 = should_exit.clone();
    let mut main_loop = || {
        println!("main loop!");
        let mut input_state = InputState::default();

        let start = time::precise_time_ns();
        for event in events.poll_iter() {
            if let Event::Quit{..} = event {
                should_exit.set(true);
                return;
            }
            let change = match states.last_mut() {
                Some(state) => state.input(event, &mut input_state),
                None => {
                    should_exit.set(true);
                    return;
                }
            };
            update_state(&mut states, change, &mut world, &mut board_id);
        }

        if let Some(state) = states.last_mut() {
            let change = state.tick(
                &mut world,
                &world_path,
                &input_state,
                &mut counters,
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
    };

    #[cfg(target_os = "emscripten")]
    emscripten::set_main_loop_callback(|| {
        if should_exit2.get() {
            exit(0);
        }
        main_loop();
    });

    #[cfg(not(target_os = "emscripten"))]
    while !should_exit2.get() {
        main_loop();
    }
}

fn main() {
    println!("main!");
    env_logger::init();
    /*let args: Vec<_> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: cargo run /path/to/world.mzx [board id]")
    } else {
        run(Path::new(&args[1]), args.get(2).and_then(|a| a.parse().ok()));
    }*/
    run(Path::new("btb/BERNARD.MZX"), None);
}
