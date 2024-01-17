use mlua::prelude::*;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::{AsyncValues, ThreadId};

pub type MessageSender = UnboundedSender<Message>;
pub type MessageReceiver = UnboundedReceiver<Message>;

#[derive(Debug)]
pub enum Message {
    Resume(ThreadId, LuaResult<AsyncValues>),
    Cancel(ThreadId),
    WriteError(LuaError),
    WriteStdout(Vec<u8>),
    WriteStderr(Vec<u8>),
}
