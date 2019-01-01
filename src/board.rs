use crate::game::{GameStateChange, reset_view};
use crate::robot::{update_robot, send_robot_to_label, BuiltInLabel, Robots, RobotId};
use libmzx::{
    KeyPress, WorldState, Counters, Board, Robot, RunStatus, Coordinate, Explosion, ExtendedParam,
    ExplosionResult, adjust_coordinate, Thing, CardinalDirection, ExtendedColorValue,
};
use num_traits::ToPrimitive;
use std::path::Path;

pub(crate) fn update_board(
    state: &mut WorldState,
    key: Option<KeyPress>,
    world_path: &Path,
    counters: &mut Counters,
    board: &mut Board,
    board_id: usize,
    all_robots: &mut Vec<Robot>,
) -> Option<GameStateChange> {
    let mut robots = Robots::new(board, all_robots);
    robots.foreach(|robot, _| {
        robot.status = RunStatus::NotRun;
    });

    let change = update_robot(
        state,
        key,
        world_path,
        counters,
        board,
        board_id,
        robots,
        RobotId::Global,
    );
    if change.is_some() {
        return change;
    }

    for y in 0..board.height {
        for x in 0..board.width {
            let coord = Coordinate(x as u16, y as u16);
            match board.thing_at(&coord) {
                Thing::Robot | Thing::RobotPushable => {
                    let robots = Robots::new(board, all_robots);

                    let change = update_robot(
                        state,
                        key,
                        world_path,
                        counters,
                        board,
                        board_id,
                        robots,
                        RobotId::from(board.level_at(&coord).2)
                    );
                    if change.is_some() {
                        return change;
                    }
                }

                Thing::Explosion => {
                    let mut explosion = Explosion::from_param(board.level_at(&coord).2);
                    if explosion.stage == 0 {
                        if explosion.size > 0 {
                            explosion.size -= 1;
                            board.level_at_mut(&coord).2 = explosion.to_param();

                            let dirs = [
                                CardinalDirection::North,
                                CardinalDirection::South,
                                CardinalDirection::East,
                                CardinalDirection::West,
                            ];
                            for dir in &dirs {
                                let adjusted = adjust_coordinate(
                                    coord,
                                    board,
                                    *dir,
                                );
                                let coord = match adjusted {
                                    Some(coord) => coord,
                                    None => continue,
                                };
                                let thing = board.thing_at(&coord);
                                if !thing.is_solid() && thing != Thing::Explosion {
                                    board.put_at(
                                        &coord,
                                        Thing::Explosion.to_u8().unwrap(),
                                        0x00,
                                        explosion.to_param(),
                                    );
                                } else if thing.is_robot() {
                                    let robot_id = RobotId::from(board.level_at(&coord).2);

                                    let mut robots = Robots::new(board, all_robots);
                                    let robot = robots.get_mut(robot_id);
                                    send_robot_to_label(robot, BuiltInLabel::Bombed);
                                }
                                // TODO: hurt player.
                            }
                        }
                    }

                    if explosion.stage == 3 {
                        let (thing, color) = match board.explosion_result {
                            ExplosionResult::Nothing => (
                                Thing::Space.to_u8().unwrap(),
                                0x07,
                            ),
                            ExplosionResult::Ash => (
                                Thing::Floor.to_u8().unwrap(),
                                0x08,
                            ),
                            ExplosionResult::Fire => (
                                Thing::Fire.to_u8().unwrap(),
                                0x0C,
                            ),
                        };
                        board.put_at(&coord, thing, color, 0x00);
                    } else {
                        explosion.stage += 1;
                        board.level_at_mut(&coord).2 = explosion.to_param();
                    }
                }

                Thing::Fire => {
                    if rand::random::<u8>() >= 20 {
                        let cur_param = board.level_at(&coord).2;
                        if cur_param < 5 {
                            board.level_at_mut(&coord).2 += 1;
                        } else {
                            board.level_at_mut(&coord).2 = 0;
                        }
                    }

                    let rval = rand::random::<u8>();
                    if rval < 8 {
                        if rval == 1 && !board.fire_burns_forever {
                            board.put_at(
                                &coord,
                                Thing::Floor.to_u8().unwrap(),
                                0x08,
                                0x00,
                            );
                        }

                        let dirs = [
                            CardinalDirection::North,
                            CardinalDirection::South,
                            CardinalDirection::East,
                            CardinalDirection::West,
                        ];
                        for dir in &dirs {
                            let adjusted = adjust_coordinate(
                                coord,
                                board,
                                *dir,
                            );
                            let coord = match adjusted {
                                Some(coord) => coord,
                                None => continue,
                            };

                            let thing = board.thing_at(&coord);
                            let level = board.level_at(&coord);
                            let thing_id = level.0;

                            let spread =
                                (thing == Thing::Space && board.fire_burns_space) ||
                                (thing_id >= Thing::Fake.to_u8().unwrap() &&
                                 thing_id <= Thing::ThickWeb.to_u8().unwrap() &&
                                 board.fire_burns_fakes) ||
                                (thing == Thing::Tree && board.fire_burns_trees) ||
                                (level.1 == 0x06 &&
                                 board.fire_burns_brown &&
                                 thing_id < Thing::Sensor.to_u8().unwrap());

                            if spread {
                                board.put_at(
                                    &coord,
                                    Thing::Fire.to_u8().unwrap(),
                                    0x0C,
                                    0x00,
                                );
                            }
                        }
                    }
                }

                _ => (),
            }
        }
    }

    state.message_color += 1;
    if state.message_color > 0x0F {
        state.message_color = 0x01;
    }
    if board.remaining_message_cycles > 0 {
        board.remaining_message_cycles -= 1;
    }

    None
}

pub(crate) fn enter_board(board: &mut Board, player_pos: Coordinate<u16>, robots: &mut [Robot]) {
    let old_pos = board.player_pos;
    if old_pos != player_pos {
        board.move_level_to(&old_pos, &player_pos);
    }
    board.player_pos = player_pos;
    reset_view(board);

    Robots::new(board, robots).foreach(|robot, _id| {
        send_robot_to_label(robot, BuiltInLabel::JustEntered);
    })
}

pub(crate) fn put_thing(
    board: &mut Board,
    color: ExtendedColorValue,
    thing: Thing,
    param: ExtendedParam,
    pos: Coordinate<u16>
) {
    let color = match color {
        ExtendedColorValue::Known(c) =>
            c.0,
        // TODO: have a table of default foreground colors for things,
        //       get the current background color at destination.
        ExtendedColorValue::Unknown(Some(_), None) |
        ExtendedColorValue::Unknown(None, Some(_)) |
        ExtendedColorValue::Unknown(None, None) |
        ExtendedColorValue::Unknown(Some(_), Some(_)) =>
            0x07, //HACK
    };

    // TODO: have a table of default parameters for things.
    let param = match param {
        ExtendedParam::Specific(p) => p.0,
        ExtendedParam::Any => 0x00, //HACK
    };

    board.put_at(&pos, thing.to_u8().unwrap(), color, param);
}
