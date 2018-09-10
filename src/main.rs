extern crate libmzx;
extern crate sdl2;

use libmzx::{Renderer, render, load_world, CardinalDirection, Coordinate, Board};
use sdl2::event::Event;
use sdl2::keyboard::{Keycode, Mod};
//use sdl2::mouse::Cursor;
use sdl2::pixels::Color;
//use sdl2::rect::Rect;
use sdl2::render::Canvas;
//use sdl2::surface::Surface;
use sdl2::video::Window;
use std::env;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::exit;
use std::time::Duration;

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
}

struct InputState {
    left_pressed: bool,
    right_pressed: bool,
    up_pressed: bool,
    down_pressed: bool,
    space_pressed: bool,
    _to_process: Option<Keycode>,
}

fn handle_key_input(
    input_state: &mut InputState,
    _timestamp: u32,
    keycode: Option<Keycode>,
    _keymod: Mod,
    _repeat: bool,
    down: bool
) {
    let keycode = match keycode {
        Some(k) => k,
        None => return,
    };
    match keycode {
        Keycode::Up => input_state.up_pressed = down,
        Keycode::Down => input_state.down_pressed = down,
        Keycode::Left => input_state.left_pressed = down,
        Keycode::Right => input_state.right_pressed = down,
        Keycode::Space => input_state.space_pressed = down,
        _ => (),
    }
}

enum InputResult {
    ExitBoard(CardinalDirection),
    Collide(Coordinate<u16>),
}

fn process_input(
    board: &mut Board,
    input_state: &InputState,
) -> Option<InputResult> {
    let player_pos = board.player_pos;
    let xdiff  = if input_state.left_pressed {
        if player_pos.0 > 0 {
            -1i8
        } else {
            return Some(InputResult::ExitBoard(CardinalDirection::West));
        }
    } else if input_state.right_pressed {
        if (player_pos.0 as usize) < board.width - 1 {
            1i8
        } else {
            return Some(InputResult::ExitBoard(CardinalDirection::East));
        }
    } else {
        0i8
    };

    let ydiff  = if xdiff == 0 && input_state.up_pressed {
        if (player_pos.1 as usize) > 0 {
            -1
        } else {
            return Some(InputResult::ExitBoard(CardinalDirection::North));
        }
    } else if xdiff == 0 && input_state.down_pressed {
        if (player_pos.1 as usize) < board.height - 1 {
            1
        } else {
            return Some(InputResult::ExitBoard(CardinalDirection::South));
        }
    } else {
        0
    };

    let new_player_pos = Coordinate(
        (player_pos.0 as i16 + xdiff as i16) as u16,
        (player_pos.1 as i16 + ydiff as i16) as u16
    );
    if new_player_pos != player_pos {
        let thing = board.thing_at(&new_player_pos);
        if thing.is_solid() {
            return Some(InputResult::Collide(new_player_pos));
        }
        board.move_level(&player_pos, xdiff, ydiff);
        board.player_pos = new_player_pos;
    }

    None
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

    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();
    let window = video_subsystem.window("revenge of megazeux", 800, 600)
      .position_centered()
      .build()
      .unwrap();

    let mut canvas = window.into_canvas().software().build().unwrap();

    canvas.clear();
    canvas.present();

    canvas.set_draw_color(Color::RGBA(255, 255, 255, 255));

    let mut events = sdl_context.event_pump().unwrap();

    const BOARD_ID: usize = 0;

    let mut input_state = InputState {
        left_pressed: false,
        right_pressed: false,
        up_pressed: false,
        down_pressed: false,
        space_pressed: false,
        _to_process: None,
    };

    'mainloop: loop {
        for event in events.poll_iter() {
            match event {
                Event::Quit{..} |
                Event::KeyDown {keycode: Option::Some(Keycode::Escape), ..} =>
                    break 'mainloop,
                Event::KeyDown {timestamp, keycode, keymod, repeat, ..} => {
                    handle_key_input(&mut input_state, timestamp, keycode, keymod, repeat, true);
                }
                Event::KeyUp {timestamp, keycode, keymod, repeat, ..} => {
                    handle_key_input(&mut input_state, timestamp, keycode, keymod, repeat, false);
                }
                _ => {}
            }
        }

        let _result = process_input(&mut world.boards[BOARD_ID], &input_state);

        {
            let mut renderer = SdlRenderer {
                canvas: &mut canvas,
            };
            render(&world.state, &world.boards[BOARD_ID], &world.board_robots[BOARD_ID], &mut renderer);
        }
        canvas.present();
        ::std::thread::sleep(Duration::new(0, 1_000_000_000u32 / 60));
    }
}

fn main() {
    let args: Vec<_> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: cargo run /path/to/world.mzx")
    } else {
        run(Path::new(&args[1]));
    }
}
