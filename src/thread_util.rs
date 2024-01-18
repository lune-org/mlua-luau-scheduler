use mlua::prelude::*;

/**
    Wrapper struct to accept either a Lua thread or a Lua function as function argument.

    [`LuaThreadOrFunction::into_thread`] may be used to convert the value into a Lua thread.
*/
#[derive(Clone)]
pub enum LuaThreadOrFunction<'lua> {
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

/**
    Trait for any struct that can be turned into an [`LuaThread`]
    and given to the scheduler, implemented for the following types:

    - Lua threads ([`LuaThread`])
    - Lua functions ([`LuaFunction`])
    - Lua chunks ([`LuaChunk`])
*/
pub trait IntoLuaThread<'lua> {
    /**
        Converts the value into a Lua thread.
    */
    fn into_lua_thread(self, lua: &'lua Lua) -> LuaResult<LuaThread<'lua>>;
}

impl<'lua> IntoLuaThread<'lua> for LuaThread<'lua> {
    fn into_lua_thread(self, _: &'lua Lua) -> LuaResult<LuaThread<'lua>> {
        Ok(self)
    }
}

impl<'lua> IntoLuaThread<'lua> for LuaFunction<'lua> {
    fn into_lua_thread(self, lua: &'lua Lua) -> LuaResult<LuaThread<'lua>> {
        lua.create_thread(self)
    }
}

impl<'lua, 'a> IntoLuaThread<'lua> for LuaChunk<'lua, 'a> {
    fn into_lua_thread(self, lua: &'lua Lua) -> LuaResult<LuaThread<'lua>> {
        lua.create_thread(self.into_function()?)
    }
}
