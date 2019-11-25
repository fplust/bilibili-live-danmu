use bilibili_live_danmu::{BMessage, Room};
use browsercookie::{Browser, Browsercookies};
use chrono::{Local, TimeZone};
use clap::{App, Arg};
use regex::Regex;
use std::sync::mpsc;
use std::thread;
use std::time;
// use ansi_term::Colour;
use cursive::align::HAlign;
use cursive::traits::*;
use cursive::view::ScrollStrategy;
use cursive::views::{BoxView, Dialog, DummyView, EditView, LinearLayout, ScrollView, TextView};
use cursive::event::Key::{Esc, Enter};
use cursive::Cursive;

// fn user_color(dan: &Danmaku) -> Colour {
//     if dan.is_admin {
//         Colour::Red
//     } else if dan.guard > 0 {
//         Colour::Blue
//     } else {
//         Colour::Green
//     }
// }

fn process_message(msg: BMessage) -> Option<String> {
    match msg {
        BMessage::DANMAKU(danmu) => {
            if danmu.is_gift {
                return None;
            }
            let date_time = Local.timestamp(danmu.timestamp, 0);
            Some(format!(
                "[{}] {}: {}",
                date_time.format("%Y-%m-%d %H:%M:%S").to_string(),
                danmu.username,
                danmu.messages,
                // user_color(&danmu).bold().paint(danmu.username),
                // Colour::White.paint(danmu.messages)
            ))
        }
        BMessage::GIFT(gift) => {
            Some(format!(
                "{}: {}{}{}",
                // Colour::Red.bold().paint(gift.username),
                gift.username,
                gift.action,
                gift.amount,
                gift.gift
            ))
        }
        BMessage::SuperChat(superchat) => {
            Some(format!(
                "{}: {}元 {}",
                superchat.username,
                superchat.price,
                superchat.message,
            ))
        }
        BMessage::BMSG(bmsg) => {
            match bmsg.cmd.as_ref() {
                "ROOM_RANK" => {
                    let data = bmsg.data.unwrap();
                    Some(format!(
                        "{}",
                        // Colour::Yellow.bold().paint(
                        //     data.get("rank_desc").unwrap().as_str().unwrap())
                        data.get("rank_desc").unwrap().as_str().unwrap()
                    ))
                }
                _ => {
                    // return Some(format!("{}", bmsg.cmd));
                    None
                }
            }
        }
    }
}

fn get_danmu(tx: mpsc::Sender<String>, room: Room) -> Result<(), std::io::Error> {
    thread::Builder::new()
        .name("danmu".into())
        .spawn(move || loop {
            for danmu in room.messages() {
                if let Some(json) = danmu {
                    let message = process_message(json);
                    if let Some(m) = message {
                        tx.send(m).unwrap();
                        thread::sleep(time::Duration::from_millis(50));
                    }
                }
            }
        })?;
    Ok(())
}

fn send_danmu(rx: mpsc::Receiver<String>, room: Room) -> Result<(), std::io::Error> {
    thread::Builder::new().name("danmu".into()).spawn(move || {
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
        for m in rx.iter() {
            room.send(&m, &cookie_header, csrf).unwrap();
        }
    })?;
    Ok(())
}

fn main() {
    let version = env!("CARGO_PKG_VERSION");
    let matches = App::new("bilibili-live-danmu")
        .version(version)
        .author("fplust. <fplustlu@gmail.com>")
        .about("bilibili 直播间弹幕机")
        .arg(
            Arg::with_name("ID")
                .required(true)
                .multiple(false)
                .help("直播间 id")
                .index(1),
        )
        .get_matches();
    let roomid: i32 = matches
        .value_of("ID")
        .unwrap()
        .parse()
        .expect("房间号需为整数");
    // println!("{}", roomid);

    let room = Room::new(roomid);

    let (tx, rx) = mpsc::channel();
    get_danmu(tx, room).unwrap();

    let (tx_s, rx_s) = mpsc::channel();
    send_danmu(rx_s, room).unwrap();

    let tx_s1 = tx_s.clone();

    let mut siv = Cursive::default();
    siv.add_layer(BoxView::with_fixed_size(
        (100, 30),
        Dialog::new()
            .title(format!("bilibili live room: {}", roomid))
            .content(
                LinearLayout::vertical()
                    .child(
                        ScrollView::new(
                            LinearLayout::vertical()
                                .child(DummyView.fixed_height(1))
                                .with(|messages| {
                                    for _ in 0..23 {
                                        messages.add_child(DummyView.fixed_height(1));
                                    }
                                })
                                .child(DummyView.fixed_height(1))
                                .with_id("messages"),
                        )
                        .scroll_strategy(ScrollStrategy::StickToBottom),
                    )
                    .child(EditView::new().with_id("message")),
            )
            .h_align(HAlign::Center)
            .button("Send", move |s| {
                let message = s
                    .call_on_id("message", |view: &mut EditView| view.get_content())
                    .unwrap();
                if !message.is_empty() {
                    tx_s1.send(message.to_string()).unwrap();
                    s.call_on_id("message", |view: &mut EditView| view.set_content(""))
                        .unwrap();
                }
            })
            .button("Quit", |s| s.quit()),
    ));

    siv.add_global_callback(Esc, |s| s.quit());
    let tx_s2 = tx_s.clone();
    siv.add_global_callback(Enter, move |s| {
        let message = s
            .call_on_id("message", |view: &mut EditView| view.get_content())
            .unwrap();
        if !message.is_empty() {
            tx_s2.send(message.to_string()).unwrap();
            s.call_on_id("message", |view: &mut EditView| view.set_content(""))
                .unwrap();
        }
    });

    siv.focus_id("message").unwrap();

    let mut message_count = 0;
    siv.refresh();

    loop {
        siv.step();
        if !siv.is_running() {
            break;
        }

        let mut needs_refresh = false;
        //Non blocking channel receiver.
        for m in rx.try_iter() {
            siv.call_on_id("messages", |messages: &mut LinearLayout| {
                needs_refresh = true;
                message_count += 1;
                messages.add_child(TextView::new(m));
                if message_count <= 24 || message_count >= 100 {
                    messages.remove_child(0);
                }
            });
        }
        if needs_refresh {
            siv.refresh();
        }
    }
}
