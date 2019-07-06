use bincode;
use byteorder::{BigEndian, ReadBytesExt};
use chrono::{Local, TimeZone};
use clap::{App, Arg};
use serde::{Deserialize, Serialize};
use serde_json;
use std::convert::TryInto;
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time;
use websocket::{
    client::sync::Client,
    // stream::sync::NetworkStream,
    stream::sync::TcpStream,
    stream::sync::TlsStream,
    ClientBuilder,
    Message,
    OwnedMessage,
};

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
            roomid,
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
            dtype,
            c: 1,
        }
    }
}

#[derive(Serialize)]
struct Pkg {
    header: Header,
    body: Vec<u8>,
}

impl Pkg {
    fn new(body: Vec<u8>, dtype: i32) -> Self {
        let header = Header::new(body.len().try_into().unwrap(), dtype);
        Self { header, body }
    }

    fn into_bytes(self) -> Vec<u8> {
        let mut config = bincode::config();
        config.big_endian();
        let header_bytes = config.serialize(&self.header).unwrap();
        header_bytes.into_iter().chain(self.body).collect()
        // bincode::serialize(self).unwrap()
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Danmu {
    cmd: String,
    data: Option<serde_json::Value>,
    info: Option<serde_json::Value>,
}

type RClient = Arc<Mutex<Client<TlsStream<TcpStream>>>>;

struct Room {
    roomid: i32,
    // client: Client<Box<dyn NetworkStream + Send>>,
    client: RClient,
    // receiver: Reader<TcpStream>,
    beat_thread: thread::JoinHandle<()>,
    // pkg: Vec<u8>,
    // heart: Vec<u8>,
}

impl Room {
    fn new(roomid: i32) -> Self {
        // Create an insecure (plain TCP) connection to the client. In this case no Box will be used,
        // you will just get a TcpStream, giving you the ability to split the stream into a reader and writer (since SSL streams cannot be cloned).
        // let client = ClientBuilder::new("ws://broadcastlv.chat.bilibili.com:2244/sub")
        //     .unwrap()
        //     .connect_insecure()
        //     .unwrap();
        let c = ClientBuilder::new("wss://broadcastlv.chat.bilibili.com/sub")
            .unwrap()
            .connect_secure(None)
            .unwrap();
        c.set_nonblocking(true).unwrap();
        let client = Arc::new(Mutex::new(c));
        // let (receiver, mut sender) = client.split().unwrap();
        let obj = Obj::new(roomid);
        let obj_bytes = serde_json::to_vec(&obj).unwrap();

        let heart = Pkg::new(b"[object Object]".to_vec(), 2).into_bytes();
        let pkg = Pkg::new(obj_bytes, 7).into_bytes();

        let sender = client.clone();
        {
            sender
                .lock()
                .unwrap()
                .send_message(&Message::binary(pkg))
                .unwrap();
        }

        let beat_thread = thread::spawn(move || {
            let messages = Message::binary(heart);
            loop {
                {
                    let lock = sender.lock();
                    if let Ok(mut s) = lock {
                        if let Ok(_) = s.send_message(&messages) {
                            // println!("send heart beat!");
                        } else {
                            println!("send heart beat error!");
                        };
                    } else {
                        println!("sender get lock error!");
                    }
                }
                thread::sleep(time::Duration::from_secs(30));
            }
        });

        Self {
            roomid,
            client,
            // receiver,
            beat_thread,
            // pkg: pkg,
            // heart: heart,
        }
    }

    fn messages(&mut self) -> impl Iterator<Item = Option<Danmu>> + '_ {
        Reciver::new(self.client.clone())
            .into_iter()
            .flat_map(|msg| Msg::new(msg).into_iter())
    }
}

struct Reciver {
    client: RClient,
}

impl Reciver {
    fn new(client: RClient) -> Self {
        Self { client }
    }
}

impl IntoIterator for Reciver {
    type Item = OwnedMessage;
    type IntoIter = RecIntoIterator;

    fn into_iter(self) -> Self::IntoIter {
        RecIntoIterator {
            client: self.client,
        }
    }
}

struct RecIntoIterator {
    client: RClient,
}

impl Iterator for RecIntoIterator {
    type Item = OwnedMessage;
    fn next(&mut self) -> Option<Self::Item> {
        let mut msg = None;
        while msg.is_none() {
            {
                let lock = self.client.lock();
                if let Ok(mut s) = lock {
                    let recv = s.recv_message();
                    // println!("{:?}", recv);
                    msg = recv.ok();
                // println!("get messages ok!");
                } else {
                    println!("receiver get lock error!");
                }
            }
            thread::sleep(time::Duration::from_secs(1));
        }
        msg
    }
}

struct Msg {
    msg: OwnedMessage,
}

impl Msg {
    fn new(msg: OwnedMessage) -> Self {
        Self { msg }
    }
}

impl IntoIterator for Msg {
    type Item = Option<Danmu>;
    type IntoIter = MsgIntoIterator;

    fn into_iter(self) -> Self::IntoIter {
        MsgIntoIterator {
            msg: self.msg,
            offset: 0,
            index: 0,
        }
    }
}

struct MsgIntoIterator {
    msg: OwnedMessage,
    offset: usize,
    index: usize,
}

impl Iterator for MsgIntoIterator {
    type Item = Option<Danmu>;
    fn next(&mut self) -> Option<Self::Item> {
        match &self.msg {
            OwnedMessage::Binary(data) => {
                let len = data.len();
                let mut rdr = Cursor::new(data);
                if self.offset < len {
                    let of: u64 = self.offset.try_into().unwrap();
                    rdr.set_position(of);
                    let mut end: usize = rdr.read_i32::<BigEndian>().unwrap().try_into().unwrap();
                    end += self.offset;
                    rdr.set_position(of + 4);
                    let mut start: usize = rdr.read_i16::<BigEndian>().unwrap().try_into().unwrap();
                    start += self.offset;
                    rdr.set_position(of + 8);
                    let dt = rdr.read_i32::<BigEndian>().unwrap();
                    self.offset = end;
                    // println!("{} {} {} {} {}", len, of, start, end, dt);
                    if dt == 5 {
                        let data = rdr.get_ref();
                        let section = &data[start..end];
                        let json: Danmu = serde_json::from_slice(section).unwrap();
                        // process_message(json);
                        // println!("{}", json);
                        self.index += 1;
                        Some(Some(json))
                    } else {
                        self.index += 1;
                        Some(None)
                    }
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

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

    let mut room = Room::new(roomid);

    for danmu in room.messages() {
        if let Some(json) = danmu {
            process_message(json)
        }
    }
}
