use std::time::Duration;

use mlua::prelude::*;
use tokio::{
    sync::mpsc::{UnboundedReceiver, UnboundedSender},
    time::Instant,
};

use crate::{AsyncValues, ThreadId};

pub type MessageSender = UnboundedSender<Message>;
pub type MessageReceiver = UnboundedReceiver<Message>;

pub enum Message {
    Resume(ThreadId, LuaResult<AsyncValues>),
    Cancel(ThreadId),
    Sleep(ThreadId, Instant, Duration),
    WriteError(LuaError),
    WriteStdout(Vec<u8>),
    WriteStderr(Vec<u8>),
}
