use chrono::{Local, TimeZone};
use clap::{App, Arg};
use bilibili_live_danmu::{
    Room,
    Danmu,
};

fn process_message(json: Danmu) {
    match json.cmd.as_ref() {
        "DANMU_MSG" => {
            let info = json.info.unwrap();
            let info = info.as_array().unwrap();
            let danmu = &info[1].as_str().unwrap();
            let ts = &info.last().unwrap().get("ts").unwrap();
            let date_time = Local.timestamp(ts.as_i64().unwrap(), 0);
            let user = &info[2][1].as_str().unwrap();
            println!(
                "[{}] {}: {}",
                date_time.format("%Y-%m-%d %H:%M:%S").to_string(),
                user,
                danmu
            );
        }
        "SEND_GIFT" => {
            let data = json.data.unwrap();
            let action = data.get("action").unwrap().as_str().unwrap();
            let giftname = data.get("giftName").unwrap().as_str().unwrap();
            let user = data.get("uname").unwrap().as_str().unwrap();
            println!("{}: {} {}", user, action, giftname);
        }
        "ROOM_RANK" => {
            let data = json.data.unwrap();
            println!("{}", data.get("rank_desc").unwrap().as_str().unwrap());
        }
        _ => {
            println!("{}", json.cmd);
        }
    }
}

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

    let room = Room::new(roomid);

    for danmu in room.messages() {
        if let Some(json) = danmu {
            process_message(json);
        }
    }
}
