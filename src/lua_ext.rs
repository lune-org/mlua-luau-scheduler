use std::future::Future;

use mlua::prelude::*;
use tokio::{spawn, task::spawn_local};

use crate::{AsyncValues, Message, MessageSender, ThreadId};

pub trait LuaAsyncExt<'lua> {
    fn current_thread_id(&'lua self) -> ThreadId;

    fn create_async_function<A, R, F, FR>(&'lua self, f: F) -> LuaResult<LuaFunction<'lua>>
    where
        A: FromLuaMulti<'lua>,
        R: Into<AsyncValues> + Send + 'static,
        F: Fn(&'lua Lua, A) -> FR + 'static,
        FR: Future<Output = LuaResult<R>> + Send + 'static;

    fn create_local_async_function<A, R, F, FR>(&'lua self, f: F) -> LuaResult<LuaFunction<'lua>>
    where
        A: FromLuaMulti<'lua>,
        R: Into<AsyncValues> + 'static,
        F: Fn(&'lua Lua, A) -> FR + 'static,
        FR: Future<Output = LuaResult<R>> + 'static;
}

impl<'lua> LuaAsyncExt<'lua> for Lua {
    fn current_thread_id(&'lua self) -> ThreadId {
        ThreadId::from(self.current_thread())
    }

    fn create_async_function<A, R, F, FR>(&'lua self, f: F) -> LuaResult<LuaFunction<'lua>>
    where
        A: FromLuaMulti<'lua>,
        R: Into<AsyncValues> + Send + 'static,
        F: Fn(&'lua Lua, A) -> FR + 'static,
        FR: Future<Output = LuaResult<R>> + Send + 'static,
    {
        let tx = self.app_data_ref::<MessageSender>().unwrap().clone();

        self.create_function(move |lua, args: A| {
            let thread_id = lua.current_thread_id();
            let fut = f(lua, args);
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

    fn create_local_async_function<A, R, F, FR>(&'lua self, f: F) -> LuaResult<LuaFunction<'lua>>
    where
        A: FromLuaMulti<'lua>,
        R: Into<AsyncValues> + 'static,
        F: Fn(&'lua Lua, A) -> FR + 'static,
        FR: Future<Output = LuaResult<R>> + 'static,
    {
        let tx = self.app_data_ref::<MessageSender>().unwrap().clone();

        self.create_function(move |lua, args: A| {
            let thread_id = lua.current_thread_id();
            let fut = f(lua, args);
            let tx = tx.clone();

            spawn_local(async move {
                tx.send(match fut.await {
                    Ok(args) => Message::Resume(thread_id, Ok(args.into())),
                    Err(e) => Message::Resume(thread_id, Err(e)),
                })
            });

            Ok(())
        })
    }
}
