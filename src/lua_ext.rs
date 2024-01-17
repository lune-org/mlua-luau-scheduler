use std::future::Future;

use mlua::prelude::*;
use tokio::spawn;

use crate::{AsyncValues, Message, MessageSender, ThreadId};

pub trait LuaSchedulerExt<'lua> {
    fn create_async_function<A, R, F, FR>(&'lua self, func: F) -> LuaResult<LuaFunction<'lua>>
    where
        A: FromLuaMulti<'lua> + 'static,
        R: Into<AsyncValues> + Send + 'static,
        F: Fn(&'lua Lua, A) -> FR + 'static,
        FR: Future<Output = LuaResult<R>> + Send + 'static;
}

impl<'lua> LuaSchedulerExt<'lua> for Lua {
    fn create_async_function<A, R, F, FR>(&'lua self, func: F) -> LuaResult<LuaFunction<'lua>>
    where
        A: FromLuaMulti<'lua> + 'static,
        R: Into<AsyncValues> + Send + 'static,
        F: Fn(&'lua Lua, A) -> FR + 'static,
        FR: Future<Output = LuaResult<R>> + Send + 'static,
    {
        let tx = self.app_data_ref::<MessageSender>().unwrap().clone();

        self.create_function(move |lua, args: A| {
            let thread_id = ThreadId::from(lua.current_thread());
            let fut = func(lua, args);
            let tx = tx.clone();

            spawn(async move {
                tx.send(match fut.await {
                    Ok(args) => Message::Resume(thread_id, Ok(args.into())),
                    Err(e) => Message::Resume(thread_id, Err(e)),
                })
            });

            Ok(())
        })
    }
}
