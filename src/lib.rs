pub mod msgs;
use bytes::{BytesMut, BufMut};
use byteorder::{
    BigEndian, ReadBytesExt
};
// use chrono::Local;
use serde::Serialize;
use std::convert::TryInto;
use std::io::Cursor;
use std::time;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
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
    error::Error,
};
use std::io::Read;
use flate2::read::ZlibDecoder;
use msgs::{ BMsg, BMessage };
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DanmukuError {
    #[error("parse error")]
    ParseError,
    #[error(transparent)]
    IOError(#[from] std::io::Error),
    #[error(transparent)]
    WebSocketError(#[from] Error),
    #[error("Package size can not exceed i32 max")]
    TooBigPkg,
    #[error("internal error")]
    InternalError,
}



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

#[derive(Serialize, Clone)]
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

#[derive(Serialize, Clone)]
struct Pkg {
    header: Header,
    body: Vec<u8>,
}

impl Pkg {
    fn new(body: Vec<u8>, dtype: i32) -> Result<Self, DanmukuError> {
        let len = body.len().try_into().or(Err(DanmukuError::TooBigPkg))?;
        let header = Header::new(len, dtype);
        Ok(Self { header, body })
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



#[derive(Clone, Copy)]
pub struct Room {
    roomid: i32,
}

pub struct MsgStream {
    _heart_beat: Option<task::JoinHandle<()>>,
    stop: Arc<AtomicBool>,
    pub stream: Box<dyn stream::Stream<Item=Result<BMessage, DanmukuError>> + Unpin>,
}

impl Drop for MsgStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if self._heart_beat.is_some() {
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

    pub async fn messages(&self) -> Result<MsgStream, DanmukuError> {
        let url = url::Url::parse("wss://broadcastlv.chat.bilibili.com/sub").or(Err(DanmukuError::InternalError))?;
        // let url = url::Url::parse("ws://broadcastlv.chat.bilibili.com:2244/sub").unwrap();
        let (ws_stream, _) = connect_async(url).await.or(Err(DanmukuError::InternalError))?;
        let (mut sender, receiver) = ws_stream.split();
        let obj = Obj::new(self.roomid);
        let obj_bytes = serde_json::to_vec(&obj).or(Err(DanmukuError::InternalError))?;

        let pkg = Pkg::new(obj_bytes, 7)?.into_bytes();

        sender.send(Message::binary(pkg)).await.expect("Failed to send message");

        let stop1 = Arc::new(AtomicBool::new(false));
        let stop2 = stop1.clone();
        let heart = Pkg::new(b"[object Object]".to_vec(), 2)?.into_bytes();
        let hb_message = Message::binary(heart);
        let _heart_beat = task::spawn(async move {
            while !stop2.load(Ordering::Relaxed) {
                if (sender.send(hb_message.clone()).await).is_ok() {
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
            let mut msgs = Vec::new();
            match msg {
                Ok(Message::Binary(data)) => {
                    msgs.append(&mut parse_pkg(data));
                },
                Err(e) => msgs.push(Err(e.into())),
                _ => {}
            }
            stream::iter(msgs)
        });
        Ok(MsgStream {
            _heart_beat: Some(_heart_beat),
            stop: stop1,
            stream: Box::new(msgs.flatten()),
        })
    }

}

#[derive(Debug)]
struct MsgHeader{
    end: usize,
    start: usize,
    ct: i16,
    dt: i32
}

fn parse_msgheader(rdr: &mut Cursor<Vec<u8>>, offset: usize) -> Result<MsgHeader, DanmukuError> {
    let of: u64 = offset as u64;
    rdr.set_position(of);
    let end: usize = rdr.read_i32::<BigEndian>()? as usize;
    rdr.set_position(of + 4);
    let start: usize = rdr.read_i16::<BigEndian>()? as usize;
    rdr.set_position(of + 6);
    let ct = rdr.read_i16::<BigEndian>()?;
    rdr.set_position(of + 8);
    let dt = rdr.read_i32::<BigEndian>()?;
    Ok(MsgHeader {
        end,
        start,
        ct,
        dt
    })
}

fn parse_pkg(data: Vec<u8>) -> Vec<Result<BMessage, DanmukuError>> {
    let len = data.len();
    // println!("{:?}", data);
    // println!("{:?}", data.clone().iter().map(|x| std::char::from_u32(*x as u32).unwrap()).collect::<Vec<char>>());
    let mut rdr = Cursor::new(data);
    let mut offset = 0;
    let mut msgs = Vec::new();
    while offset < len {
        match parse_msgheader(&mut rdr, offset) {
            Ok(header) => {
                let start = header.start + offset;
                let end = header.end + offset;
                offset = end;
                // dbg!(len, &header, offset);
                if header.dt == 5 {
                    let data = rdr.get_ref();
                    let section = &data[start..end];
                    if header.ct == 2 {
                        let mut buf = Vec::new();
                        let mut deflater = ZlibDecoder::new(section);
                        match deflater.read_to_end(&mut buf) {
                            Ok(_) => msgs.append(&mut parse_pkg(buf)),
                            Err(_) => msgs.push(Err(DanmukuError::ParseError))
                        }
                        // json = serde_json::from_slice(&buf[16..]).ok();
                    } else {
                        // dbg!(std::str::from_utf8(section));
                        match serde_json::from_slice::<BMsg>(section) {
                            Ok(json) => {
                                msgs.push(Ok(json.into()))
                            },
                            Err(_) => msgs.push(Err(DanmukuError::ParseError))
                        }
                    }
                }
            }
            Err(e) => {
                msgs.push(Err(e));
                break
            }
        };
    }
    msgs
}
