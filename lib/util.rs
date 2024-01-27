use futures_lite::StreamExt;
use mlua::prelude::*;

/**
    Runs a Lua thread until it manually yields (using coroutine.yield), errors, or completes.

    Returns the values yielded by the thread, or the error that caused it to stop.
*/
pub(crate) async fn run_until_yield<'lua>(
    thread: LuaThread<'lua>,
    args: LuaMultiValue<'lua>,
) -> LuaResult<LuaMultiValue<'lua>> {
    let mut stream = thread.into_async(args);
    /*
        NOTE: It is very important that we drop the thread/stream as
        soon as we are done, it takes up valuable Lua registry space
        and detached tasks will not drop until the executor does

        https://github.com/smol-rs/smol/issues/294
    */
    stream.next().await.unwrap()
}

/**
    Representation of a [`LuaThread`] with its associated arguments currently stored in the Lua registry.
*/
#[derive(Debug)]
pub(crate) struct ThreadWithArgs {
    key_thread: LuaRegistryKey,
    key_args: LuaRegistryKey,
}

impl ThreadWithArgs {
    pub fn new<'lua>(
        lua: &'lua Lua,
        thread: LuaThread<'lua>,
        args: LuaMultiValue<'lua>,
    ) -> LuaResult<Self> {
        let argsv = args.into_vec();

        let key_thread = lua.create_registry_value(thread)?;
        let key_args = lua.create_registry_value(argsv)?;

        Ok(Self {
            key_thread,
            key_args,
        })
    }

    pub fn into_inner(self, lua: &Lua) -> (LuaThread<'_>, LuaMultiValue<'_>) {
        let thread = lua.registry_value(&self.key_thread).unwrap();
        let argsv = lua.registry_value(&self.key_args).unwrap();

        let args = LuaMultiValue::from_vec(argsv);

        lua.remove_registry_value(self.key_thread).unwrap();
        lua.remove_registry_value(self.key_args).unwrap();

        (thread, args)
    }
}

/**
    Wrapper struct to accept either a Lua thread or a Lua function as function argument.

    [`LuaThreadOrFunction::into_thread`] may be used to convert the value into a Lua thread.
*/
#[derive(Clone)]
pub(crate) enum LuaThreadOrFunction<'lua> {
    Thread(LuaThread<'lua>),
    Function(LuaFunction<'lua>),
}

impl<'lua> LuaThreadOrFunction<'lua> {
    pub(super) fn into_thread(self, lua: &'lua Lua) -> LuaResult<LuaThread<'lua>> {
        match self {
            Self::Thread(t) => Ok(t),
            Self::Function(f) => lua.create_thread(f),
        }
    }
}

impl<'lua> FromLua<'lua> for LuaThreadOrFunction<'lua> {
    fn from_lua(value: LuaValue<'lua>, _: &'lua Lua) -> LuaResult<Self> {
        match value {
            LuaValue::Thread(t) => Ok(Self::Thread(t)),
            LuaValue::Function(f) => Ok(Self::Function(f)),
            value => Err(LuaError::FromLuaConversionError {
                from: value.type_name(),
                to: "LuaThreadOrFunction",
                message: Some("Expected thread or function".to_string()),
            }),
        }
    }
}
