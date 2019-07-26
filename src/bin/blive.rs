use ansi_term::Colour;
use bilibili_live_danmu::{BMessage, Danmaku, Room};
use browsercookie::{Browser, Browsercookies};
use chrono::{Local, TimeZone};
use clap::{App, Arg, SubCommand};
use regex::Regex;
use rustyline::error::ReadlineError;
use rustyline::Editor;
use std::thread;
use std::time;

fn user_color(dan: &Danmaku) -> Colour {
    if dan.is_admin {
        Colour::Red
    } else if dan.guard > 0 {
        Colour::Blue
    } else {
        Colour::Green
    }
}

fn process_message(msg: BMessage) {
    match msg {
        BMessage::DANMAKU(danmu) => {
            if danmu.is_gift {
                return;
            }
            let date_time = Local.timestamp(danmu.timestamp, 0);
            println!(
                "[{}] {}: {}",
                date_time.format("%Y-%m-%d %H:%M:%S").to_string(),
                user_color(&danmu).bold().paint(danmu.username),
                Colour::White.paint(danmu.messages)
            );
            thread::sleep(time::Duration::from_millis(50));
        }
        BMessage::GIFT(gift) => {
            println!(
                "{}: {}{}{}",
                Colour::Red.bold().paint(gift.username),
                gift.action,
                gift.amount,
                gift.gift
            );
        }
        BMessage::BMSG(bmsg) => {
            match bmsg.cmd.as_ref() {
                "ROOM_RANK" => {
                    let data = bmsg.data.unwrap();
                    println!(
                        "{}",
                        Colour::Yellow
                            .bold()
                            .paint(data.get("rank_desc").unwrap().as_str().unwrap())
                    );
                }
                _ => {
                    // println!("{}", bmsg.cmd);
                }
            }
        }
    }
}

fn main() {
    let matches = App::new("bilibili-live-danmu")
        .version("0.3.0")
        .author("fplust. <fplustlu@gmail.com>")
        .about("bilibili 直播间弹幕机")
        .subcommand(
            SubCommand::with_name("send").about("发送弹幕").arg(
                Arg::with_name("ID")
                    .required(true)
                    .multiple(false)
                    .help("直播间 id")
                    .index(1),
            ),
        )
        .subcommand(
            SubCommand::with_name("view").about("查看弹幕").arg(
                Arg::with_name("ID")
                    .required(true)
                    .multiple(false)
                    .help("直播间 id")
                    .index(1),
            ),
        )
        .get_matches();
    if let Some(matches) = matches.subcommand_matches("send") {
        let roomid: i32 = matches
            .value_of("ID")
            .unwrap()
            .parse()
            .expect("房间号需为整数");
        // println!("{}", roomid);
        let mut bc = Browsercookies::new();
        let domain_regex = Regex::new("bilibili").unwrap();
        bc.from_browser(Browser::Firefox, &domain_regex)
            .expect("Failed to get firefox browser cookies");
        // println!("{:?}", &bc.cj);
        let csrf = bc
            .cj
            .get("bili_jct")
            .expect("请使用firefox登录后重试")
            .value();
        // println!("{:?}", csrf);

        let cookie_header = bc.to_header(&domain_regex).unwrap();

        let room = Room::new(roomid);

        let mut rl = Editor::<()>::new();
        loop {
            let readline = rl.readline("输入要发送的弹幕: ");
            match readline {
                Ok(line) => {
                    println!("发送: {}", line);
                    let res = room.send(&line, &cookie_header, csrf).unwrap();
                    println!("{}", res);
                }
                Err(ReadlineError::Interrupted) => {
                    println!("CTRL-C");
                    break;
                }
                Err(ReadlineError::Eof) => {
                    println!("CTRL-D");
                    break;
                }
                Err(err) => {
                    println!("Error: {:?}", err);
                    break;
                }
            }
        }
    }
    if let Some(matches) = matches.subcommand_matches("view") {
        let roomid: i32 = matches
            .value_of("ID")
            .unwrap()
            .parse()
            .expect("房间号需为整数");
        // println!("{}", roomid);

        let room = Room::new(roomid);

        loop {
            for danmu in room.messages() {
                if let Some(json) = danmu {
                    process_message(json);
                }
            }
        }
    }
}
