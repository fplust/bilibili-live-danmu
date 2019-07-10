use bincode;
use byteorder::{BigEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};
use serde_json;
use std::convert::TryInto;
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use std::thread;
use std::sync::mpsc;
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
use reqwest::header;
use chrono::Local;

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

#[derive(Debug)]
pub struct Danmaku {
    pub uid: i64,
    pub username: String,
    pub messages: String,
    pub guard: i64,
    pub is_admin: bool,
    pub timestamp: i64,
}

#[derive(Debug)]
pub struct Gift {
    pub uid: i64,
    pub username: String,
    pub action: String,
    pub gift: String,
    pub amount: i64,
    pub value: i64,
    pub guard_type: i64,
}

#[derive(Debug)]
pub enum BMessage {
    DANMAKU(Danmaku),
    GIFT(Gift),
    BMSG(BMsg),
}

impl From<BMsg> for BMessage {
    fn from(msg: BMsg) -> Self {
        match msg.cmd.as_ref() {
            "DANMU_MSG" => {
                let info = msg.info.unwrap();
                BMessage::DANMAKU(Danmaku {
                    uid: info[2][0].as_i64().unwrap(),
                    username: info[2][1].as_str().unwrap().to_string(),
                    messages: info[1].as_str().unwrap().to_string(),
                    guard: info[7].as_i64().unwrap(),
                    is_admin: info[2][2].as_i64().unwrap() == 1,
                    timestamp: info.as_array().unwrap().last().unwrap()
                        .get("ts").unwrap().as_i64().unwrap(),
                })
            }
            "SEND_GIFT" => {
                let data = msg.data.unwrap();
                let mut value = data.get("total_coin").unwrap().as_i64().unwrap();
                let coin_type = data.get("coin_type").unwrap();
                if coin_type != "gold" {
                    value *= 0;
                }
                BMessage::GIFT(Gift {
                    uid: data.get("uid").unwrap().as_i64().unwrap(),
                    username: data.get("uname").unwrap().as_str().unwrap().to_string(),
                    action: data.get("action").unwrap().as_str().unwrap().to_string(),
                    gift: data.get("giftName").unwrap().as_str().unwrap().to_string(),
                    amount: data.get("num").unwrap().as_i64().unwrap(),
                    value: value / 1000,
                    guard_type: 0,
                })
            }
            "GUARD_BUY" => {
                let data = msg.data.unwrap();
                let value = data.get("price").unwrap().as_i64().unwrap();
                BMessage::GIFT(Gift {
                    uid: data.get("uid").unwrap().as_i64().unwrap(),
                    username: data.get("username").unwrap().as_str().unwrap().to_string(),
                    action: "购买".into(),
                    gift: data.get("gift_name").unwrap().as_str().unwrap().to_string(),
                    amount: data.get("num").unwrap().as_i64().unwrap(),
                    value: value / 1000,
                    guard_type: data.get("guard_level").unwrap().as_i64().unwrap(),
                })
            }
            _ => {
                BMessage::BMSG(msg)
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BMsg {
    pub cmd: String,
    pub data: Option<serde_json::Value>,
    pub info: Option<serde_json::Value>,
}

type RClient = Arc<Mutex<Client<TlsStream<TcpStream>>>>;

pub struct Room {
    roomid: i32,
    // client: Client<Box<dyn NetworkStream + Send>>,
    // receiver: Reader<TcpStream>,
    // pkg: Vec<u8>,
    // heart: Vec<u8>,
}

impl Room {
    pub fn new(roomid: i32) -> Self {
        Self {
            roomid,
        }
    }

    pub fn send(&self, msg: &str, cookies: &str, csrf_token: &str) -> Result<serde_json::Value, String> {
        let mut headers = header::HeaderMap::new();
        headers.insert(header::COOKIE, header::HeaderValue::from_str(&cookies).unwrap());

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build().unwrap();
        let now = Local::now().timestamp();
        let params = [
            ("color", "16777215"),
            ("fontsize", "25"),
            ("mode", "1"),
            ("msg", msg),
            ("rnd", &format!("{}", now)),
            ("roomid", &format!("{}", self.roomid)),
            ("bubble", "0"),
            ("csrf_token", csrf_token),
            ("csrf", csrf_token),
        ];
        let res: serde_json::Value = client.post("https://api.live.bilibili.com/msg/send")
            .form(&params)
            .send().unwrap().json().unwrap();
        // println!("{}", res);
        Ok(res)
    }

    pub fn messages(&self) -> impl Iterator<Item = Option<BMessage>> + '_ {
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
        let obj = Obj::new(self.roomid);
        let obj_bytes = serde_json::to_vec(&obj).unwrap();

        let pkg = Pkg::new(obj_bytes, 7).into_bytes();

       client 
            .lock()
            .unwrap()
            .send_message(&Message::binary(pkg))
            .unwrap();

        Reciver::new(client)
            .into_iter()
            .flat_map(|msg| Msg::new(msg).into_iter())
    }
}

struct Reciver {
    client: RClient,
    beat_thread: thread::JoinHandle<()>,
    tx: mpsc::Sender<()>,
}

impl Reciver {
    fn new(client: RClient) -> Self {
        let sender = client.clone();
        let heart = Pkg::new(b"[object Object]".to_vec(), 2).into_bytes();
        let (tx, rx) = mpsc::channel();
        let beat_thread = thread::Builder::new().name("heart_beat".into()).spawn(move || {
            let messages = Message::binary(heart);
            loop {
                if let Ok(_) = rx.try_recv() {
                    // println!("stop heart beat!");
                    break;
                } else {
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
            }
        }).unwrap();

        Self { client, beat_thread, tx }
    }
}

impl Drop for Reciver {
    fn drop(&mut self) {
        self.tx.send(()).unwrap();
    }
}

impl IntoIterator for Reciver {
    type Item = OwnedMessage;
    type IntoIter = RecIntoIterator;

    fn into_iter(self) -> Self::IntoIter {
        RecIntoIterator {
            receiver: self,
        }
    }
}

struct RecIntoIterator {
    receiver: Reciver,
}

impl Iterator for RecIntoIterator {
    type Item = OwnedMessage;
    fn next(&mut self) -> Option<Self::Item> {
        let mut msg = None;
        loop {
            {
                let lock = self.receiver.client.lock();
                if let Ok(mut s) = lock {
                    let recv = s.recv_message();
                    // println!("{:?}", recv);
                    msg = recv.ok();
                // println!("get messages ok!");
                } else {
                    println!("receiver get lock error!");
                }
            }
            if msg.is_none() {
                thread::sleep(time::Duration::from_millis(100));
            } else {
                return msg;
            }
        }
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
    type Item = Option<BMessage>;
    type IntoIter = MsgIntoIterator;

    fn into_iter(self) -> Self::IntoIter {
        MsgIntoIterator {
            msg: self.msg,
            offset: 0,
        }
    }
}

struct MsgIntoIterator {
    msg: OwnedMessage,
    offset: usize,
}

impl Iterator for MsgIntoIterator {
    type Item = Option<BMessage>;
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
                        let json: BMsg = serde_json::from_slice(section).unwrap();
                        // process_message(json);
                        // println!("{}", json);
                        Some(Some(json.into()))
                    } else {
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
