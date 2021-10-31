use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
pub struct UserInfo {
    pub face: String,
    pub uname: String
}

#[derive(Debug, Deserialize)]
pub struct SuperChat {
    pub uid: i64,
    pub user_info: UserInfo,
    pub message: String,
    pub message_trans: String,
    pub price: i64,
}

#[derive(Debug)]
#[non_exhaustive]
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
                let msg: SuperChat = serde_json::from_value(data).unwrap();
                BMessage::SuperChat(msg)
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