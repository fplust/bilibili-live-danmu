use bytes::{BytesMut, BufMut};
use byteorder::{
    BigEndian, ReadBytesExt
};
use chrono::Local;
use reqwest::header;
use serde::{Deserialize, Serialize};
use serde_json;
use std::convert::TryInto;
use std::io::Cursor;
use std::time;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use url;
use tokio::task;
use tokio::time::sleep;
use tokio_stream as stream;
use async_tungstenite::tokio::{
    connect_async,
};
use futures::{
    SinkExt,
    StreamExt,
};
use async_tungstenite::tungstenite::{
    protocol::Message,
    // error::Error,
};
use libflate::zlib::Decoder;
use std::io::Read;

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
            protover: 2,
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
        // let config = bincode::options();
        // config.with_big_endian();
        // let header_bytes = config.serialize(&self.header).unwrap();
        let mut header_bytes = BytesMut::with_capacity(16);
        header_bytes.put_i32(self.header.len);
        header_bytes.put_i16(self.header.a);
        header_bytes.put_i16(self.header.b);
        header_bytes.put_i32(self.header.dtype);
        header_bytes.put_i32(self.header.c);
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
    pub user_level: i64,
    pub is_gift: bool,
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
pub struct SuperChat {
    pub uid: i64,
    pub username: String,
    pub message: String,
    pub message_jpn: String,
    pub price: i64,
}

#[derive(Debug)]
pub enum BMessage {
    DANMAKU(Danmaku),
    GIFT(Gift),
    SuperChat(SuperChat),
    BMSG(BMsg),
}

#[inline]
fn v2string(value: &serde_json::Value) -> String {
    value.as_str().unwrap().to_string()
}

impl From<BMsg> for BMessage {
    fn from(msg: BMsg) -> Self {
        match msg.cmd.as_str() {
            "DANMU_MSG" => {
                let info = msg.info.unwrap();
                BMessage::DANMAKU(Danmaku {
                    uid: info[2][0].as_i64().unwrap(),
                    username: v2string(&info[2][1]),
                    messages: v2string(&info[1]),
                    guard: info[7].as_i64().unwrap(),
                    is_admin: info[2][2].as_i64().unwrap() == 1,
                    timestamp: info[9]
                        .get("ts")
                        .unwrap()
                        .as_i64()
                        .unwrap(),
                    user_level: info[4][0].as_i64().unwrap(),
                    is_gift: info[0][9].as_i64().unwrap() > 0,
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
                    username: v2string(&data.get("uname").unwrap()),
                    action: v2string(&data.get("action").unwrap()),
                    gift: v2string(&data.get("giftName").unwrap()),
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
                    username: v2string(&data.get("username").unwrap()),
                    action: "购买".into(),
                    gift: v2string(&data.get("gift_name").unwrap()),
                    amount: data.get("num").unwrap().as_i64().unwrap(),
                    value: value / 1000,
                    guard_type: data.get("guard_level").unwrap().as_i64().unwrap(),
                })
            }
            // "SUPER_CHAT_MESSAGE" | "SUPER_CHAT_MESSAGE_JPN" => {
            "SUPER_CHAT_MESSAGE" => {
                let data = msg.data.unwrap();
                BMessage::SuperChat(SuperChat {
                    uid: data.get("uid").unwrap().as_i64().unwrap(),
                    username: v2string(&data.get("user_info").unwrap().get("uname").unwrap()),
                    message: v2string(&data.get("message").unwrap()),
                    message_jpn: v2string(&data.get("message_trans").unwrap()),
                    price: data.get("price").unwrap().as_i64().unwrap(),
                })
            }
            _ => BMessage::BMSG(msg),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BMsg {
    pub cmd: String,
    pub data: Option<serde_json::Value>,
    pub info: Option<serde_json::Value>,
}


#[derive(Clone, Copy)]
pub struct Room {
    roomid: i32,
}

pub struct MsgStream {
    _heart_beat: Option<task::JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    pub stream: Box<dyn stream::Stream<Item=BMessage> + Unpin>,
}

impl Drop for MsgStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if self._heart_beat.is_some() {
            // let mut rt = tokio::runtime::Runtime::new().unwrap();
            let heart_beat = self._heart_beat.take().unwrap();
            tokio::spawn(async move {
                heart_beat.await.unwrap();
            });
        }
    }
}

impl Room {
    pub fn new(roomid: i32) -> Self {
        Self { roomid }
    }

    pub async fn send(
        &self,
        msg: &str,
        cookies: &str,
        csrf_token: &str,
    ) -> Result<serde_json::Value, String> {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::COOKIE,
            header::HeaderValue::from_str(&cookies).unwrap(),
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .unwrap();
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
        let res: serde_json::Value = client
            .post("https://api.live.bilibili.com/msg/send")
            .form(&params)
            .send().await
            .unwrap()
            .json().await
            .unwrap();
        // println!("{}", res);
        Ok(res)
    }

    pub async fn messages(&self) -> MsgStream {
        let url = url::Url::parse("wss://broadcastlv.chat.bilibili.com/sub").unwrap();
        // let url = url::Url::parse("ws://broadcastlv.chat.bilibili.com:2244/sub").unwrap();
        let (ws_stream, _) = connect_async(url).await.expect("Failed to connect");
        let (mut sender, receiver) = ws_stream.split();
        let obj = Obj::new(self.roomid);
        let obj_bytes = serde_json::to_vec(&obj).unwrap();

        let pkg = Pkg::new(obj_bytes, 7).into_bytes();

        sender.send(Message::binary(pkg)).await.expect("Failed to send message");

        let stop1 = Arc::new(AtomicBool::new(false));
        let stop2 = stop1.clone();
        let _heart_beat = task::spawn(async move {
            while !stop2.load(Ordering::Relaxed) {
                let heart = Pkg::new(b"[object Object]".to_vec(), 2).into_bytes();
                let messages = Message::binary(heart);
                if let Ok(_) = sender.send(messages).await {
                    #[cfg(debug_assertions)]
                    dbg!("send heart beat!");
                } else {
                    println!("send heart beat error!");
                };
                sleep(time::Duration::from_secs(10)).await;
            }
            println!("heart beat stop");
        });

        let msgs = receiver.map(|msg| {
            let mut msgs: Vec<BMessage> = Vec::new();
            match msg.unwrap() {
                Message::Binary(data) => {
                    parse_pkg(data, &mut msgs);
                }
                _ => {},
            }
            stream::iter(msgs)
        });
        MsgStream {
            _heart_beat: Some(_heart_beat),
            stop: stop1,
            stream: Box::new(msgs.flatten()),
        }
    }
}

fn parse_pkg(data: Vec<u8>, msgs: &mut Vec<BMessage>) {
    let len = data.len();
    // println!("{:?}", data);
    // println!("{:?}", data.clone().iter().map(|x| std::char::from_u32(*x as u32).unwrap()).collect::<Vec<char>>());
    let mut rdr = Cursor::new(data);
    let mut offset = 0;
    while offset < len {
        let of: u64 = offset as u64;
        rdr.set_position(of);
        let mut end: usize = rdr.read_i32::<BigEndian>().unwrap() as usize;
        end += offset;
        rdr.set_position(of + 4);
        let mut start: usize = rdr.read_i16::<BigEndian>().unwrap() as usize;
        start += offset;
        rdr.set_position(of + 6);
        let ct = rdr.read_i16::<BigEndian>().unwrap();
        rdr.set_position(of + 8);
        let dt = rdr.read_i32::<BigEndian>().unwrap();
        offset = end;
        // println!("{} {} {} {} {} {}", len, of, start, end, ct, dt);
        if dt == 5 {
            let data = rdr.get_ref();
            let section = &data[start..end];
            if ct == 2 {
                let mut decoder = Decoder::new(section).unwrap();
                let mut buf = Vec::new();
                decoder.read_to_end(&mut buf).unwrap();
                parse_pkg(buf, msgs);
                // println!("{:?}", buf);
                // json = serde_json::from_slice(&buf[16..]).ok();
            } else {
                let json: Option<BMsg> = serde_json::from_slice(section).ok();
                match json {
                    Some(j) => {
                        // println!("{:?}", j);
                        msgs.push(j.into());
                    }
                    None => {
                        // println!("解析失败!");
                    }
                }
            }
        }
    }
}
