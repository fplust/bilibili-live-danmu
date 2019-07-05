use std::convert::TryInto;
use std::thread::sleep;
use std::io::Cursor;
use std::time;
use clap::{App, Arg};
use websocket::{ClientBuilder, Message, OwnedMessage};
use serde::{Deserialize, Serialize};
use serde_json;
use bincode;
use byteorder::{BigEndian, ReadBytesExt};

#[derive(Serialize)]
struct Obj {
    uid: i32,
    roomid: i32,
    protover: i32,
    platform: String,
    clientver: String,
}

impl Obj {
    fn new(roomid: i32) -> Self {
        Self {
            uid: 0,
            roomid: roomid,
            protover: 1,
            platform: String::from("web"),
            clientver: String::from("1.5.15"),
        }
    }
}

#[derive(Serialize)]
struct Header {
    len: i32,
    a: i16,
    b: i16,
    dtype: i32,
    c: i32,
}

impl Header {
    fn new(len: i32, dtype: i32) -> Self {
        Self {
            len: 16 + len,
            a: 16,
            b: 1,
            dtype: dtype,
            c: 1
        }
    }
}

#[derive(Serialize)]
struct Pkg {
    header: Header,
    body: Vec<u8>
}

#[derive(Debug, Serialize, Deserialize)]
struct Danmu {
    cmd: String,
    data: Option<serde_json::Value>,
    info: Option<serde_json::Value>,
}

impl Pkg {
    fn new(body: Vec<u8>, dtype: i32) -> Self {
        let header = Header::new(body.len().try_into().unwrap(), dtype);
        Self {
            header: header,
            body: body,
        }
    }

    fn to_bytes(self) -> Vec<u8> {
        let mut config = bincode::config();
        config.big_endian();
        let header_bytes = config.serialize(&self.header).unwrap();
        header_bytes.into_iter().chain(self.body).collect()
        // bincode::serialize(self).unwrap()
    }
}

fn process_message(json: Danmu) {
    match json.cmd.as_ref() {
        "DANMU_MSG" => {
            let info = json.info.unwrap();
            let danmu = &info[1].as_str().unwrap();
            let user = &info[2][1].as_str().unwrap();
            println!("{}: {}", user, danmu);
        },
        "SEND_GIFT" => {
            let data = json.data.unwrap();
            let action = data.get("action").unwrap().as_str().unwrap();
            let giftname = data.get("giftName").unwrap().as_str().unwrap();
            let user = data.get("uname").unwrap().as_str().unwrap();
            println!("{}: {} {}", user, action, giftname);
        },
        "ROOM_RANK" => {
            let data = json.data.unwrap();
            println!("{}", data.get("rank_desc").unwrap().as_str().unwrap());
        },
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
        .arg(Arg::with_name("ID")
             .required(true)
             .multiple(false)
             .help("直播间 id")
             .index(1))
        .get_matches();
    let roomid: i32 = matches.value_of("ID").unwrap().parse().unwrap();
    // println!("{}", roomid);

    let mut client = ClientBuilder::new("wss://broadcastlv.chat.bilibili.com/sub").unwrap().connect(None).unwrap();

    let obj = Obj::new(roomid);
    let obj_bytes = serde_json::to_vec(&obj).unwrap();

    let heart = Pkg::new("[object Object]".as_bytes().to_vec(), 2).to_bytes();
    let pkg = Pkg::new(obj_bytes, 7).to_bytes();
    // let pkg_str = String::from_utf8(pkg).unwrap();
    // let heart_str = String::from_utf8(heart).unwrap();
    // println!("{:?}", pkg);
    // println!("{:?}", heart);
    client.send_message(&Message::binary(pkg)).unwrap();
    client.send_message(&Message::binary(heart)).unwrap();
    // client.send_message(&Message::text(pkg_str)).unwrap();
    // client.send_message(&Message::text(heart_str)).unwrap();
    for msg in client.incoming_messages() {
        match msg {
            Ok(m) => {
                match m {
                    OwnedMessage::Binary(data) => {
                        let len = data.len();
                        let mut rdr = Cursor::new(data);
                        let mut offset: usize = 0;
                        while offset < len {
                            let of: u64 = offset.try_into().unwrap();
                            rdr.set_position(of);
                            let mut end: usize = rdr.read_i32::<BigEndian>().unwrap().try_into().unwrap();
                            end = offset + end;
                            rdr.set_position(of + 4);
                            let mut start: usize = rdr.read_i16::<BigEndian>().unwrap().try_into().unwrap();
                            start = offset + start;
                            rdr.set_position(of + 8);
                            let dt = rdr.read_i32::<BigEndian>().unwrap();
                            offset = end;
                            // println!("{} {} {} {} {}", len, of, start, end, dt);
                            if dt == 5 {
                                let data = rdr.get_ref();
                                let section = &data[start..end];
                                let json: Danmu = serde_json::from_slice(section).unwrap();
                                process_message(json);
                                // println!("{}", json);
                            }
                        }
                        // println!("{:?}", rdr.get_ref());
                    },
                    _ => panic!()
                }
            },
            Err(_) => {}
        }
        sleep(time::Duration::from_secs(1));
    }
}
