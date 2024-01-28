#![allow(unused_imports)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::module_name_repetitions)]

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use event_listener::Event;
use mlua::prelude::*;

use crate::{
    runtime::Runtime,
    status::Status,
    traits::IntoLuaThread,
    util::{run_until_yield, ThreadWithArgs},
};

/**
    A handle to a thread that has been spawned onto a [`Runtime`].

    This handle contains a public method, [`Handle::result`], which may
    be used to extract the result of the thread, once it finishes running.

    A result may be waited for using the [`Handle::listen`] method.
*/
#[derive(Debug, Clone)]
pub struct Handle {
    thread: Rc<RefCell<Option<ThreadWithArgs>>>,
    result: Rc<RefCell<Option<(bool, LuaRegistryKey)>>>,
    status: Rc<Cell<bool>>,
    event: Rc<Event>,
}

impl Handle {
    pub(crate) fn new<'lua>(
        lua: &'lua Lua,
        thread: impl IntoLuaThread<'lua>,
        args: impl IntoLuaMulti<'lua>,
    ) -> LuaResult<Self> {
        let thread = thread.into_lua_thread(lua)?;
        let args = args.into_lua_multi(lua)?;

        let packed = ThreadWithArgs::new(lua, thread, args)?;

        Ok(Self {
            thread: Rc::new(RefCell::new(Some(packed))),
            result: Rc::new(RefCell::new(None)),
            status: Rc::new(Cell::new(false)),
            event: Rc::new(Event::new()),
        })
    }

    pub(crate) fn create_thread<'lua>(&self, lua: &'lua Lua) -> LuaResult<LuaThread<'lua>> {
        let env = lua.create_table()?;
        env.set("handle", self.clone())?;
        lua.load("return handle:resume()")
            .set_name("__runtime_handle")
            .set_environment(env)
            .into_lua_thread(lua)
    }

    fn take<'lua>(&self, lua: &'lua Lua) -> (LuaThread<'lua>, LuaMultiValue<'lua>) {
        self.thread
            .borrow_mut()
            .take()
            .expect("thread handle may only be taken once")
            .into_inner(lua)
    }

    fn set<'lua>(
        &self,
        lua: &'lua Lua,
        result: &LuaResult<LuaMultiValue<'lua>>,
        is_final: bool,
    ) -> LuaResult<()> {
        self.result.borrow_mut().replace((
            result.is_ok(),
            match &result {
                Ok(v) => lua.create_registry_value(v.clone().into_vec())?,
                Err(e) => lua.create_registry_value(e.clone())?,
            },
        ));
        self.status.replace(is_final);
        if is_final {
            self.event.notify(usize::MAX);
        }
        Ok(())
    }

    /**
        Extracts the result for this thread handle.

        Depending on the current [`Runtime::status`], this method will return:

        - [`Status::NotStarted`]: returns `None`.
        - [`Status::Running`]: may return `Some(Ok(v))` or `Some(Err(e))`, but it is not guaranteed.
        - [`Status::Completed`]: returns `Some(Ok(v))` or `Some(Err(e))`.
    */
    #[must_use]
    pub fn result<'lua>(&self, lua: &'lua Lua) -> Option<LuaResult<LuaMultiValue<'lua>>> {
        let res = self.result.borrow();
        let (is_ok, key) = res.as_ref()?;
        Some(if *is_ok {
            let v = lua.registry_value(key).unwrap();
            Ok(LuaMultiValue::from_vec(v))
        } else {
            Err(lua.registry_value(key).unwrap())
        })
    }

    /**
        Waits for this handle to have its final result available.

        Does not wait if the final result is already available.
    */
    pub async fn listen(&self) {
        if !self.status.get() {
            self.event.listen().await;
        }
    }
}

impl LuaUserData for Handle {
    fn add_methods<'lua, M: LuaUserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_async_method("resume", |lua, this, (): ()| async move {
            /*
                1. Take the thread and args out of the handle
                2. Run the thread until it yields or completes
                3. Store the result of the thread in the lua registry
                4. Return the result of the thread back to lua as well, so that
                   it may be caught using the runtime and any error callback(s)
            */
            let (thread, args) = this.take(lua);
            let result = run_until_yield(thread.clone(), args).await;
            let is_final = thread.status() != LuaThreadStatus::Resumable;
            this.set(lua, &result, is_final)?;
            result
        });
    }
}
