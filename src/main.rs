#![feature(nll)]

extern crate env_logger;
extern crate libmzx;
#[macro_use] extern crate log;
extern crate num_traits;
extern crate rand;
extern crate sdl2;
extern crate time;

use crate::game::{InputState, TitleState};
use libmzx::{load_world, World, Counters, Renderer};
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

fn run(world_path: &Path) {
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
    let video_subsystem = sdl_context.video().unwrap();
    let window = video_subsystem.window("revenge of megazeux", 640, 350)
      .position_centered()
      .build()
      .unwrap();

    let mut canvas = window.into_canvas().software().build().unwrap();

    canvas.clear();
    canvas.present();

    canvas.set_draw_color(Color::RGBA(255, 255, 255, 255));

    let mut states = vec![Box::new(TitleState) as Box<GameState>];

    let mut events = sdl_context.event_pump().unwrap();

    let mut board_id = 0;
    let game_speed: u64 = 4;

    let mut counters = Counters::new();

    'mainloop: loop {
        let mut input_state = InputState::default();

        let start = time::precise_time_ns();
        for event in events.poll_iter() {
            if let Event::Quit{..} = event {
                break 'mainloop;
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
    }
}

fn main() {
    env_logger::init();
    let args: Vec<_> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: cargo run /path/to/world.mzx")
    } else {
        run(Path::new(&args[1]));
    }
}
