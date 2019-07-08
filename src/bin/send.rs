use clap::{App, Arg};
use bilibili_live_danmu::{
    Room,
};
use browsercookie::{Browser, Browsercookies};
use regex::Regex;
use std::io;

fn main() {
    let matches = App::new("bilibili-live-danmu")
        .version("0.1.0")
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
    let mut bc = Browsercookies::new();
    let domain_regex = Regex::new("bilibili").unwrap();
    bc.from_browser(Browser::Firefox, &domain_regex).expect("Failed to get firefox browser cookies");
    // println!("{:?}", &bc.cj);
    let csrf = bc.cj.get("bili_jct").expect("请使用firefox登录后重试").value();
    // println!("{:?}", csrf);

    let cookie_header = bc.to_header(&domain_regex).unwrap();

    let room = Room::new(roomid);
    let inputs = io::stdin();
    loop {
        let mut msg = String::new();
        println!("输入要发送的弹幕:");
        inputs.read_line(&mut msg).unwrap();
        println!("发送: {}", msg);
        room.send(&msg, &cookie_header, csrf).unwrap();
    }
}

