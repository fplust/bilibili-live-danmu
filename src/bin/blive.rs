use ansi_term::Colour;
use blive_danmu::Room;
use blive_danmu::msgs::{BMessage, Danmaku};
// use browsercookie::{Browser, Browsercookies};
use time::{OffsetDateTime, format_description, UtcOffset};
use clap::{App, Arg, SubCommand};
// use regex::Regex;
// use rustyline::error::ReadlineError;
// use rustyline::Editor;
use std::thread::sleep;
use std::time::Duration;
use tokio::runtime;
use tokio_stream::StreamExt;

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
            let date_time = OffsetDateTime::from_unix_timestamp(danmu.timestamp).unwrap();
            let local_datetime = date_time.to_offset(UtcOffset::from_hms(8, 0, 0).unwrap());
            let format = format_description::parse("[year]-[month]-[day] [hour]:[minute]:[second]").unwrap();
            println!(
                "[{}] {}: {}",
                local_datetime.format(&format).unwrap().to_string(),
                user_color(&danmu).bold().paint(danmu.username),
                Colour::White.paint(danmu.messages)
            );
        }
        BMessage::GIFT(gift) => {
            match gift.gift.as_str() {
                "辣条" => {},
                _ => println!(
                    "{}: {}{}{}",
                    Colour::Red.bold().paint(gift.username),
                    gift.action,
                    gift.amount,
                    gift.gift
                ),
            }
        }
        BMessage::SuperChat(superchat) => {
            println!(
                "{}: {}元 {}",
                Colour::Red.bold().paint(superchat.user_info.uname),
                superchat.price,
                superchat.message,
            );
        }
        BMessage::BMSG(bmsg) => {
            match bmsg.cmd.as_str() {
                "ROOM_RANK" => {
                    let data = bmsg.data.unwrap();
                    println!(
                        "{}",
                        Colour::Yellow
                            .bold()
                            .paint(data.get("rank_desc").unwrap().as_str().unwrap())
                    );
                }
                "SUPER_CHAT_MESSAGE" => {
                    let data = bmsg.data.unwrap();
                    println!(
                        "SuperChat: {}",
                        Colour::Yellow
                            .bold()
                            .paint(format!("{:?}", data))
                    );
                }
                _ => {
                    // println!("{}", bmsg.cmd);
                }
            }
        }
        _ => {}
    }
}

fn main() {
    let version = env!("CARGO_PKG_VERSION");
    let matches = App::new("bilibili-live-danmu")
        .version(version)
        .author("fplust. <fplustlu@gmail.com>")
        .about("bilibili 直播间弹幕机")
        /*
        .subcommand(
            SubCommand::with_name("send").about("发送弹幕").arg(
                Arg::with_name("ID")
                    .required(true)
                    .multiple(false)
                    .help("直播间 id")
                    .index(1),
            ),
        )
        */
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
    /*
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
        let rt = runtime::Builder::new_current_thread().enable_all().build().unwrap();
        loop {
            let readline = rl.readline("输入要发送的弹幕: ");
            match readline {
                Ok(line) => {
                    rt.block_on(async {
                        println!("发送: {}", line);
                        let res = room.send(&line, &cookie_header, csrf).await.unwrap();
                        println!("{}", res);
                    });
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
    */
    if let Some(matches) = matches.subcommand_matches("view") {
        let roomid: i32 = matches
            .value_of("ID")
            .unwrap()
            .parse()
            .expect("房间号需为整数");
        // println!("{}", roomid);

        let room = Room::new(roomid);

        let rt = runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let mut danmus = room.messages().await.unwrap();
            println!("start danmu");
            while let Some(danmu) = danmus.stream.next().await {
                // println!("{:?}", danmu);
                process_message(danmu.unwrap());
                sleep(Duration::from_millis(50));
            }
        });
    }
}
