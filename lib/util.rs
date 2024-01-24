use std::cell::OnceCell;

use mlua::prelude::*;

use crate::IntoLuaThread;

thread_local! {
    static POLL_PENDING: OnceCell<LuaLightUserData> = OnceCell::new();
}

fn get_poll_pending(lua: &Lua) -> LuaResult<LuaLightUserData> {
    let yielder_fn = lua.create_async_function(|_, ()| async move {
        smol::future::yield_now().await;
        Ok(())
    })?;

    yielder_fn
        .into_lua_thread(lua)?
        .resume::<_, LuaLightUserData>(())
}

#[inline]
pub(crate) fn is_poll_pending(value: &LuaValue) -> bool {
    // TODO: Replace with Lua::poll_pending() when it's available

    let pp = POLL_PENDING.with(|cell| {
        *cell.get_or_init(|| {
            let lua = Lua::new().into_static();
            let pending = get_poll_pending(lua).unwrap();
            // SAFETY: We only use the Lua state for the lifetime of this function,
            // and the "poll pending" light userdata / pointer is completely static.
            drop(unsafe { Lua::from_static(lua) });
            pending
        })
    });

    matches!(value, LuaValue::LightUserData(u) if u == &pp)
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
