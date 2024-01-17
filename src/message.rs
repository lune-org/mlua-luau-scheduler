use std::time::Duration;

use mlua::prelude::*;
use tokio::{
    sync::mpsc::{UnboundedReceiver, UnboundedSender},
    time::Instant,
};

use crate::{Args, ThreadId};

pub type MessageSender = UnboundedSender<Message>;
pub type MessageReceiver = UnboundedReceiver<Message>;

pub enum Message {
    Resume(ThreadId, LuaResult<Args>),
    Cancel(ThreadId),
    Sleep(ThreadId, Instant, Duration),
    Error(ThreadId, Box<LuaError>),
    WriteStdout(Vec<u8>),
    WriteStderr(Vec<u8>),
}
